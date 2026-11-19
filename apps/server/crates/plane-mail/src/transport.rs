// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! SMTP sending, message bytes and `get_email_configuration`.
//!
//! Moved from `plane-http-app::instances::mail` (P0, `plans/CUTOVER.md` §6):
//! the `/api/instances/email-credentials-check/` handler still owns the route,
//! while Phase 8's task handlers share the transport. `plane-http-app`
//! re-exports this module, so its call sites and the golden test did not
//! change their behaviour.
//!
//! Python: `plane/license/api/views/configuration.py:80-171`
//! (`EmailCredentialCheckEndpoint.post`). The view reads seven values with
//! `get_email_configuration` (`plane/license/utils/instance_value.py:42-59`),
//! builds a connection with `django.core.mail.get_connection` and sends one
//! `EmailMultiAlternatives` through `django.core.mail.backends.smtp.EmailBackend`
//! (`django/core/mail/backends/smtp.py:70-186`).
//!
//! Nothing here is registered as a route: `instances::email` owns the handler
//! and calls [`send`].
//!
//! ## What is reproduced byte for byte
//!
//! * `get_email_configuration`'s seven values, including the environment
//!   fallbacks and the encrypted `EMAIL_HOST_PASSWORD` (`configuration_values`),
//!   `int(EMAIL_PORT)` (a non-numeric value is a `ValueError` **outside** the
//!   view's `try`, i.e. a 500), `EMAIL_FROM=""` becoming
//!   `settings.DEFAULT_FROM_EMAIL` ("webmaster@localhost"), and
//!   `EMAIL_USE_TLS == "1"` / `EMAIL_USE_SSL == "1"`.
//! * `EmailMultiAlternatives.message().as_bytes(linesep="\r\n")`: header order,
//!   the RFC 2047 base64 encoded words `forbid_multi_line_headers` produces,
//!   Python's 78-column folding (`email/header.py:_ValueFormatter`,
//!   `email/_policybase.py:348-379`), the `text/plain; charset="utf-8"` MIME
//!   shell, and the 7bit/8bit/quoted-printable body selection
//!   (`django/core/mail/message.py:172-183`). The golden corpus is
//!   `tests/golden/email_messages.json`, recorded by
//!   `tools/mail/email_message_corpus.py` from the real Django.
//! * `sanitize_address` (`django/core/mail/message.py:92-125`) and smtplib's
//!   `quoteaddr` for the `MAIL FROM:`/`RCPT TO:` envelope
//!   (`django/core/mail/backends/smtp.py:166-175`, `smtplib.py:545,553`).
//! * The connection: plaintext, STARTTLS (`use_tls`) or TLS-wrapped
//!   (`use_ssl`), with authentication only when **both** username and password
//!   are non-empty (`smtp.py:93-95`) — Django's rule, not lettre's.
//!
//! ## Known differences (recorded, not silent)
//!
//! * `socket.getfqdn()` (`django/core/mail/utils.py` `DNS_NAME`) is ported by
//!   [`getfqdn`]: `gethostname()`, then the first `getaddrinfo` answer for the
//!   name, then `getnameinfo`, then the address's `in-addr.arpa` name as the
//!   alias Apple's libinfo reports. `gethostbyaddr` itself is not reachable
//!   through a safe crate, so the alias list is not observable: a platform
//!   whose canonical name is single-label *and* whose `/etc/hosts` has a
//!   dotted alias would see Python return the alias and the port the
//!   canonical name. On Linux (glibc stops at `/etc/hosts`) and on macOS
//!   (libinfo's only extra alias is the reverse-mapping name) the port
//!   returns Python's value; see the module docs on [`getfqdn`].
//! * TLS trust. Django builds `ssl.create_default_context()` (the system
//!   store); the port uses `webpki-roots` (DECISIONS.md D9's equivalent of
//!   `certifi`, with `aws-lc-rs` as the single crypto provider).
//! * Authentication mechanism. `smtplib.login` prefers `CRAM-MD5`, then
//!   `PLAIN`, then `LOGIN` (`smtplib.py:725-750`), retrying the next one after
//!   a `535`; the port offers `PLAIN` then `LOGIN` with the same retry. A
//!   server that advertises only `CRAM-MD5` therefore fails here and may
//!   succeed in Python.
//! * `EMAIL_TIMEOUT` is unset in Plane, so Django waits forever; the port
//!   passes `None` to lettre for the same behaviour.
//!
//! ## Session model
//!
//! The transport is lettre's [`AsyncNetworkStream`] (a plaintext socket, an
//! implicit-TLS socket, or one `upgrade_tls` turns into TLS), and the SMTP
//! dialogue is written out command by command instead of using
//! `AsyncTransport::send_raw`. That is what makes each failure branch
//! observable: `send_raw` reports only a reply code, so a refused sender and
//! refused recipients are indistinguishable (`smtplib` raises
//! `SMTPSenderRefused` / `SMTPRecipientsRefused` from the two commands). The
//! sequence reproduces `smtplib.SMTP.sendmail` including its `RSET` after a
//! refusal, `size=` when the server advertises `SIZE`, `quit()` in
//! `send_messages`' `finally`, and no `QUIT` for a connection that failed
//! while opening (`connect`, `STARTTLS`, `AUTH`).

#![allow(dead_code, reason = "set C (instances routes) consumes this transport")]
#![allow(
    deprecated,
    reason = "lettre 0.11 marks `AsyncNetworkStream` deprecated but keeps it public and \
              exposes no other way to drive an SMTP session command by command (D23)"
)]

use std::error::Error as _;
use std::net::IpAddr;
use std::sync::OnceLock;

use base64::Engine as _;
use chrono::{Datelike, Timelike, Utc};
use deadpool_postgres::Client;
use dns_lookup::AddrInfoHints;
use futures_util::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use lettre::transport::smtp::client::{AsyncNetworkStream, TlsParameters};
use plane_auth::error::AuthError;
use plane_auth::instance_config::configuration_values;
use plane_core::config::Settings;

/// `settings.DEFAULT_FROM_EMAIL`'s Django default; Plane does not set it
/// (`plane/settings/common.py`), and `EmailMessage.__init__` applies it when
/// `from_email` is empty (`django/core/mail/message.py:246`).
pub const DEFAULT_FROM_EMAIL: &str = "webmaster@localhost";

/// `settings.DEFAULT_CHARSET` (`"utf-8"`; `django/conf/global_settings.py`).
const DEFAULT_CHARSET: &str = "utf-8";

/// The mail configuration `get_email_configuration` returns, after the
/// conversions the view performs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailConfiguration {
    /// `EMAIL_HOST`; `None` when neither the row nor the environment has it.
    pub host: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    /// `int(EMAIL_PORT)`. Kept as `u32` because Python checks the range only
    /// when the socket is opened (a port above 65535 is an `OverflowError`, a
    /// 400 generic error); see [`EmailConfiguration::from_values`].
    pub port: u32,
    pub use_tls: bool,
    pub use_ssl: bool,
    /// `EMAIL_FROM`, already `EMAIL_FROM or DEFAULT_FROM_EMAIL`.
    pub from: String,
}

/// Why the endpoint cannot send, in the endpoint's own branches
/// (`plane/license/api/views/configuration.py:122-171`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MailError {
    /// `BadHeaderError`: a header value contains a newline.
    BadHeader(String),
    /// `SMTPAuthenticationError`: "Invalid credentials provided".
    Authentication,
    /// `SMTPConnectError`: the server did not answer its greeting with 220.
    Connect,
    /// `SMTPSenderRefused`: "From address is invalid."
    SenderRefused,
    /// `SMTPServerDisconnected`.
    ServerDisconnected,
    /// `SMTPRecipientsRefused`: "All recipient addresses were refused."
    RecipientsRefused,
    /// `TimeoutError`.
    Timeout,
    /// `ConnectionError` (a refused or reset connection).
    Connection,
    /// Everything else the endpoint's `except Exception` catches, plus the
    /// errors it does not catch (`int(EMAIL_PORT)`, TLS+SSL both set), which
    /// Django answers with a 500.
    Generic(String),
}

impl MailError {
    /// The view's response body for this branch
    /// (`plane/license/api/views/configuration.py:122-171`), or `None` when
    /// Django would not answer with a 400 at all (a 500).
    #[must_use]
    pub fn endpoint_message(&self) -> Option<&'static str> {
        match self {
            Self::BadHeader(_) => Some("Invalid email header."),
            Self::Authentication => Some("Invalid credentials provided"),
            Self::Connect => Some("Could not connect with the SMTP server."),
            Self::SenderRefused => Some("From address is invalid."),
            Self::ServerDisconnected => Some("SMTP server disconnected unexpectedly."),
            Self::RecipientsRefused => Some("All recipient addresses were refused."),
            Self::Timeout => Some("Timeout error while trying to connect to the SMTP server."),
            Self::Connection => {
                Some("Network connection error. Please check your internet connection.")
            }
            Self::Generic(_) => Some("Could not send email. Please check your configuration"),
        }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Generic(detail) => write!(f, "{detail}"),
            other => write!(
                f,
                "{}",
                other.endpoint_message().unwrap_or("mail transport error")
            ),
        }
    }
}

impl std::error::Error for MailError {}

impl EmailConfiguration {
    /// `get_email_configuration()` + the view's conversions.
    ///
    /// `values` is [`configuration_values`]' answer for the seven keys, in
    /// `get_email_configuration` order: host, user, password, port, use_tls,
    /// use_ssl, from.
    ///
    /// `int(EMAIL_PORT)` runs **before** the view's `try`, so a non-numeric
    /// port is a `ValueError` and Django answers 500. It is reported as
    /// [`MailError::Generic`] (the handler maps it to the 500 the pipeline
    /// produces for an uncaught exception).
    pub fn from_values(values: Vec<Option<String>>) -> Result<Self, MailError> {
        let mut values = values.into_iter();
        let mut next = || values.next().unwrap_or(None);
        let host = next();
        let user = next();
        let password = next();
        let port_raw = next();
        let use_tls = next();
        let use_ssl = next();
        let from = next();

        // `int(EMAIL_PORT)`: Python accepts surrounding whitespace, a sign and
        // underscores between digits (`int("1_000")`). The default is the int
        // 587 (`instance_value.py:52`), which stringifies to "587".
        let port = py_int(port_raw.as_deref())?;
        let use_tls = use_tls.as_deref() == Some("1");
        let use_ssl = use_ssl.as_deref() == Some("1");
        // `EmailMessage.__init__`: `from_email or settings.DEFAULT_FROM_EMAIL`.
        let from = match from {
            Some(from) if !from.is_empty() => from,
            _ => DEFAULT_FROM_EMAIL.to_string(),
        };
        Ok(Self {
            host,
            user,
            password,
            port,
            use_tls,
            use_ssl,
            from,
        })
    }

    /// `get_email_configuration()` against `instance_configurations`.
    ///
    /// The defaults are the call site's own `os.environ.get(...)` fallbacks
    /// (`plane/license/utils/instance_value.py:42-59`);
    /// [`configuration_values`] applies the environment before them, as Python
    /// does in both `SKIP_ENV_VAR` branches.
    ///
    /// A database failure and an invalid port are both uncaught exceptions in
    /// Django (500), unlike [`send`]'s errors (400): the handler maps this
    /// result to the 500 path.
    pub async fn load(client: &Client, settings: &Settings) -> Result<Self, MailError> {
        let values = configuration_values(
            client,
            settings,
            &[
                ("EMAIL_HOST", None),
                ("EMAIL_HOST_USER", None),
                ("EMAIL_HOST_PASSWORD", None),
                ("EMAIL_PORT", Some("587")),
                ("EMAIL_USE_TLS", Some("1")),
                ("EMAIL_USE_SSL", Some("0")),
                ("EMAIL_FROM", Some("Team Plane <team@mailer.plane.so>")),
            ],
        )
        .await
        .map_err(|error: AuthError| MailError::Generic(error.to_string()))?;
        Self::from_values(values)
    }
}

/// Python `int()` for the strings an instance configuration can hold.
fn py_int(value: Option<&str>) -> Result<u32, MailError> {
    let Some(raw) = value else {
        return Err(MailError::Generic(
            "int() argument must be a string, a bytes-like object or a real number, not 'NoneType'"
                .to_string(),
        ));
    };
    let text = raw.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() {
        return Err(invalid_int(raw));
    }
    let mut value: u64 = 0;
    let mut previous_underscore = false;
    for (index, ch) in digits.chars().enumerate() {
        if ch == '_' {
            // Python only allows a single underscore *between* digits.
            if index == 0 || previous_underscore {
                return Err(invalid_int(raw));
            }
            previous_underscore = true;
            continue;
        }
        let Some(digit) = ch.to_digit(10) else {
            return Err(invalid_int(raw));
        };
        previous_underscore = false;
        value = value
            .checked_mul(10)
            .and_then(|value| value.checked_add(u64::from(digit)))
            .ok_or_else(|| invalid_int(raw))?;
    }
    if previous_underscore {
        return Err(invalid_int(raw));
    }
    let value = if negative {
        -(value as i64)
    } else {
        value as i64
    };
    // `socket.create_connection` raises `OverflowError` outside 1..=65535; the
    // view's `except Exception` turns it into the generic 400.
    u32::try_from(value).map_err(|_| invalid_int(raw))
}

fn invalid_int(raw: &str) -> MailError {
    MailError::Generic(format!("invalid literal for int() with base 10: {raw:?}"))
}

/// One `EmailMultiAlternatives` (`django/core/mail/message.py:454-545`).
///
/// The endpoint's messages have no alternatives; the HTML e-mail tasks attach
/// one `text/html` alternative, which produces Django's
/// `multipart/alternative` shell ([`Message::as_bytes_with_boundary`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub subject: String,
    pub body: String,
    pub from_email: String,
    /// `to`, `cc`, `reply_to` in the order the view sets them.
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub reply_to: Vec<String>,
    /// `attach_alternative(content, mimetype)` in call order.
    pub alternatives: Vec<Alternative>,
    /// `attach(filename, content)` in call order.
    pub attachments: Vec<Attachment>,
}

/// One `EmailAlternative` (`django/core/mail/message.py:239`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternative {
    pub content: String,
    /// `"text/html"` on every Plane task that attaches one.
    pub mimetype: String,
}

/// One `EmailMessage.attach(filename, content)` rendered by
/// `_create_attachment` (`django/core/mail/message.py:309-347,382-426`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: String,
    /// The guessed MIME type from the filename (`mimetypes.guess_type`).
    pub mimetype: String,
    /// The text payload Django hands to `SafeMIMEText`.
    pub content: String,
}

impl Alternative {
    /// `attach_alternative(html_content, "text/html")`.
    #[must_use]
    pub fn html(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            mimetype: "text/html".to_string(),
        }
    }
}

impl Message {
    /// The endpoint's message: fixed subject and body, the configured From and
    /// the request's `receiver_email` (`configuration.py:102-118`).
    #[must_use]
    pub fn email_credentials_check(receiver_email: &str, from: &str) -> Self {
        Self {
            subject: "Email Notification from Plane".to_string(),
            body: "This is a sample email notification sent from Plane application.".to_string(),
            from_email: from.to_string(),
            to: vec![receiver_email.to_string()],
            cc: Vec::new(),
            reply_to: Vec::new(),
            alternatives: Vec::new(),
            attachments: Vec::new(),
        }
    }

    /// A task's `EmailMultiAlternatives(..., body=text_content)` with
    /// `msg.attach_alternative(html_content, "text/html")`
    /// (`plane/bgtasks/magic_link_code_task.py:51-59`).
    #[must_use]
    pub fn with_html(
        subject: impl Into<String>,
        text: impl Into<String>,
        html: impl Into<String>,
        from: impl Into<String>,
        to: Vec<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            body: text.into(),
            from_email: from.into(),
            to,
            cc: Vec::new(),
            reply_to: Vec::new(),
            alternatives: vec![Alternative::html(html)],
            attachments: Vec::new(),
        }
    }

    /// `EmailMultiAlternatives(subject, body=text, from_email=from, to=[to])`
    /// with `msg.attach(filename, content)` for a text attachment, the shape
    /// `analytic_plot_export.send_export_email` builds
    /// (`plane/bgtasks/analytic_plot_export.py:45-88`).
    #[must_use]
    pub fn with_text_attachment(
        subject: impl Into<String>,
        text: impl Into<String>,
        from: impl Into<String>,
        to: Vec<String>,
        filename: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let filename = filename.into();
        let mimetype = match filename.rsplit_once('.').map(|(_, extension)| extension) {
            Some(extension) if extension.eq_ignore_ascii_case("csv") => "text/csv",
            Some(extension) if extension.eq_ignore_ascii_case("txt") => "text/plain",
            Some(extension) if extension.eq_ignore_ascii_case("html") => "text/html",
            _ => "application/octet-stream",
        };
        Self {
            subject: subject.into(),
            body: text.into(),
            from_email: from.into(),
            to,
            cc: Vec::new(),
            reply_to: Vec::new(),
            alternatives: Vec::new(),
            attachments: vec![Attachment {
                filename,
                mimetype: mimetype.to_string(),
                content: content.into(),
            }],
        }
    }

    /// `from_email or settings.DEFAULT_FROM_EMAIL` (`EmailMessage.__init__`,
    /// `django/core/mail/message.py:246`).
    #[must_use]
    pub fn effective_from(&self) -> &str {
        if self.from_email.is_empty() {
            DEFAULT_FROM_EMAIL
        } else {
            &self.from_email
        }
    }

    /// `EmailMessage.recipients()`: `to + cc + bcc`, empty entries dropped.
    #[must_use]
    pub fn recipients(&self) -> Vec<&str> {
        self.to
            .iter()
            .chain(self.cc.iter())
            .map(String::as_str)
            .filter(|address| !address.is_empty())
            .collect()
    }

    /// `EmailMultiAlternatives.message().as_bytes(linesep="\r\n")`.
    ///
    /// `date` and `message_id` are the two generated headers; production passes
    /// [`default_date`] and [`default_message_id`], and the unit tests pass the
    /// corpus placeholders `<DATE>` / `<MESSAGE-ID>`.
    pub fn as_bytes(&self, date: &str, message_id: &str) -> Result<Vec<u8>, MailError> {
        self.as_bytes_with_boundary(date, message_id, None)
    }

    /// [`Message::as_bytes`] with a known multipart boundary.
    ///
    /// Python's boundary is random (`email/generator.py:_make_boundary`), so a
    /// golden corpus masks it and passes the recorded value here. A message
    /// with no alternatives ignores the argument and stays `text/plain`, which
    /// is the endpoint's shape.
    pub fn as_bytes_with_boundary(
        &self,
        date: &str,
        message_id: &str,
        boundary: Option<&str>,
    ) -> Result<Vec<u8>, MailError> {
        let mut out = Vec::with_capacity(512 + self.body.len());

        // `_create_attachments` runs after `_create_alternatives`: with at
        // least one attachment it wraps the (text or alternative) message in
        // `multipart/mixed` (`django/core/mail/message.py:365-380`).
        if !self.attachments.is_empty() {
            let boundary = boundary.map_or_else(make_boundary, str::to_string);
            write_header(
                &mut out,
                "Content-Type",
                &format!("multipart/mixed; boundary=\"{boundary}\""),
            )?;
            out.extend_from_slice(b"MIME-Version: 1.0\r\n");
            out = self.write_headers_only(out, date, message_id)?;
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(b"--");
            out.extend_from_slice(boundary.as_bytes());
            out.extend_from_slice(b"\r\n");
            if self.alternatives.is_empty() {
                write_text_part(&mut out, &self.body, "plain")?;
            } else {
                write_header(
                    &mut out,
                    "Content-Type",
                    &format!("multipart/alternative; boundary=\"{boundary}\""),
                )?;
                out.extend_from_slice(b"MIME-Version: 1.0\r\n\r\n");
                write_text_part(&mut out, &self.body, "plain")?;
                for alternative in &self.alternatives {
                    out.extend_from_slice(b"\r\n--");
                    out.extend_from_slice(boundary.as_bytes());
                    out.extend_from_slice(b"\r\n");
                    let subtype = alternative
                        .mimetype
                        .split_once('/')
                        .map(|(_, subtype)| subtype)
                        .unwrap_or("html");
                    write_text_part(&mut out, &alternative.content, subtype)?;
                }
                out.extend_from_slice(b"\r\n--");
                out.extend_from_slice(boundary.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
            for attachment in &self.attachments {
                out.extend_from_slice(b"\r\n--");
                out.extend_from_slice(boundary.as_bytes());
                out.extend_from_slice(b"\r\n");
                write_attachment_part(&mut out, attachment)?;
            }
            out.extend_from_slice(b"\r\n--");
            out.extend_from_slice(boundary.as_bytes());
            out.extend_from_slice(b"--\r\n");
            return Ok(out);
        }

        // `_create_alternatives` wraps the text shell in
        // `SafeMIMEMultipart(_subtype="alternative")` before `message()` adds
        // Subject/From/To/Cc/Reply-To/Date/Message-ID
        // (`django/core/mail/message.py:454-520`).
        if self.alternatives.is_empty() {
            // `MIMEText.__init__` writes its own three headers first.
            out.extend_from_slice(b"Content-Type: text/plain; charset=\"utf-8\"\r\n");
            out.extend_from_slice(b"MIME-Version: 1.0\r\n");
            let (cte, body) = encode_text_body(&self.body);
            out.extend_from_slice(format!("Content-Transfer-Encoding: {cte}\r\n").as_bytes());
            return self.write_headers_and_body(out, date, message_id, &body);
        }

        let boundary = boundary.map_or_else(make_boundary, str::to_string);
        write_header(
            &mut out,
            "Content-Type",
            &format!("multipart/alternative; boundary=\"{boundary}\""),
        )?;
        out.extend_from_slice(b"MIME-Version: 1.0\r\n");
        out = self.write_headers_only(out, date, message_id)?;
        out.extend_from_slice(b"\r\n");
        // `Generator._handle_multipart`: every subpart is flattened on its own,
        // then joined with `--boundary` delimiters. The first delimiter comes
        // before the first part.
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\n");
        write_text_part(&mut out, &self.body, "plain")?;
        for alternative in &self.alternatives {
            out.extend_from_slice(b"\r\n--");
            out.extend_from_slice(boundary.as_bytes());
            out.extend_from_slice(b"\r\n");
            let subtype = alternative
                .mimetype
                .split_once('/')
                .map(|(_, subtype)| subtype)
                .unwrap_or("html");
            write_text_part(&mut out, &alternative.content, subtype)?;
        }
        out.extend_from_slice(b"\r\n--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"--\r\n");
        Ok(out)
    }

    /// Header block, blank line and the text/plain payload, for the
    /// non-multipart shape.
    fn write_headers_and_body(
        &self,
        mut out: Vec<u8>,
        date: &str,
        message_id: &str,
        body: &str,
    ) -> Result<Vec<u8>, MailError> {
        out = self.write_headers_only(out, date, message_id)?;
        out.extend_from_slice(b"\r\n");
        // `Generator._write_lines`: the payload's line endings become CRLF and
        // a trailing newline is not doubled.
        write_crlf_lines(&mut out, body);
        Ok(out)
    }

    /// The `EmailMessage.message()` header block, in Django's insertion order.
    fn write_headers_only(
        &self,
        mut out: Vec<u8>,
        date: &str,
        message_id: &str,
    ) -> Result<Vec<u8>, MailError> {
        // EmailMessage.message(): Subject, From, To, Cc, Reply-To, Date,
        // Message-ID, then any extra headers (none here).
        write_header(&mut out, "Subject", &self.subject)?;
        // `from_email or settings.DEFAULT_FROM_EMAIL` already happened in
        // `Message::new`'s callers; the header uses the attribute verbatim.
        write_header(&mut out, "From", self.effective_from())?;
        write_list_header(&mut out, "To", &self.to)?;
        write_list_header(&mut out, "Cc", &self.cc)?;
        write_list_header(&mut out, "Reply-To", &self.reply_to)?;
        write_header(&mut out, "Date", date)?;
        write_header(&mut out, "Message-ID", message_id)?;
        Ok(out)
    }

    /// The `(from, recipients)` smtplib puts in `MAIL FROM:`/`RCPT TO:`, with
    /// `sanitize_address` and `quoteaddr` applied
    /// (`django/core/mail/backends/smtp.py:166-175`).
    pub fn envelope(&self) -> Result<(String, Vec<String>), MailError> {
        let from = sanitize_address(self.effective_from())?;
        let from = quoteaddr(&from).ok_or_else(|| {
            MailError::Generic(format!("Invalid address {:?}", self.effective_from()))
        })?;
        let mut recipients = Vec::new();
        for address in self.recipients() {
            let sanitized = sanitize_address(address)?;
            let recipient = quoteaddr(&sanitized)
                .ok_or_else(|| MailError::Generic(format!("Invalid address {address:?}")))?;
            recipients.push(recipient);
        }
        Ok((from, recipients))
    }
}

/// `formatdate(localtime=False)` (`email/utils.py:298-340`): UTC, `-0000`
/// zone, English day/month names.
#[must_use]
pub fn default_date() -> String {
    let now = Utc::now();
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} -0000",
        DAYS[now.weekday().num_days_from_monday() as usize],
        now.day(),
        MONTHS[now.month0() as usize],
        now.year(),
        now.hour(),
        now.minute(),
        now.second(),
    )
}

/// `make_msgid(domain=DNS_NAME)` (`email/utils.py:154-177`):
/// `<{int(time.time()*100)}.{pid}.{random64}@{domain}>`.
///
/// The domain is `socket.getfqdn()` in Django (`django/core/mail/utils.py`
/// `DNS_NAME`); the caller passes the cached [`dns_name`], the same value the
/// EHLO argument uses.
#[must_use]
pub fn default_message_id(domain: &str) -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let timeval = since_epoch.as_micros() / 10_000;
    let pid = std::process::id();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap_or_default();
    let randint = u64::from_be_bytes(random);
    format!("<{timeval}.{pid}.{randint}@{domain}>")
}

// ---------------------------------------------------------------------------
// `socket.getfqdn()` (`Lib/socket.py:getfqdn`), Django's `DNS_NAME`
// ---------------------------------------------------------------------------

/// `DNS_NAME` (`django/core/mail/utils.py:10-18`): the process's FQDN,
/// computed on first use and reused by every later message, so the EHLO
/// argument and every `Message-ID` domain of a process are identical, as in
/// Django.
static DNS_NAME: OnceLock<String> = OnceLock::new();

/// The cached `socket.getfqdn()`.
pub fn dns_name() -> &'static str {
    DNS_NAME.get_or_init(|| getfqdn(""))
}

/// [`dns_name`] without blocking the runtime: the resolver calls run on
/// tokio's blocking pool the first time.
async fn dns_name_async() -> &'static str {
    if let Some(name) = DNS_NAME.get() {
        return name;
    }
    match tokio::task::spawn_blocking(dns_name).await {
        Ok(name) => name,
        // The blocking task can only fail by panicking; Django's
        // `DNS_NAME.get_fqdn()` would raise there, but a hostname is not worth
        // failing a send over.
        Err(_) => "localhost",
    }
}

/// `socket.getfqdn(name)` (`Lib/socket.py`).
///
/// CPython: strip the argument; an empty argument, `0.0.0.0` or `::` means
/// `gethostname()`; then `hostname, aliases, addresses = gethostbyaddr(name)`,
/// and the result is the first of `[hostname, *aliases]` that contains a dot,
/// else `hostname`. A failed lookup returns the argument unchanged.
///
/// `gethostbyaddr` has no safe binding, so it is reproduced with the two calls
/// CPython itself makes inside it (`Modules/socketmodule.c:setipaddr` and
/// `socket_gethostbyaddr`): `getaddrinfo(name, NULL, {AF_UNSPEC, socktype 0,
/// flags 0})` and the first answer, then `getnameinfo(address, NI_NAMEREQD)`
/// for the canonical name.
///
/// The alias list is the part a safe API does not expose. macOS's libinfo
/// reports the address's reverse-mapping name (`d.c.b.a.in-addr.arpa`,
/// `…ip6.arpa`) as an alias of every successful reverse lookup, and prefers it
/// whenever the canonical name has no dot — the only case where Python
/// consults an alias — so macOS returns that name, which is what
/// `socket.getfqdn()` returns there (`localhost` has one label, the
/// `…in-addr.arpa` name does not). glibc reports no alias for an `/etc/hosts`
/// entry and the PTR name as the canonical name otherwise, so its result is
/// the canonical name, which is what every other platform gets here. A
/// platform whose `/etc/hosts` holds a *dotted* alias for a host whose
/// canonical name is single-label is the one place this differs from Python.
pub fn getfqdn(name: &str) -> String {
    let mut name = name.trim().to_string();
    if name.is_empty() || name == "0.0.0.0" || name == "::" {
        name = hostname();
    }
    let Some(address) = first_address(&name) else {
        return name;
    };
    match dns_lookup::lookup_addr(&address) {
        Ok(canonical) => {
            if canonical.contains('.') {
                return canonical;
            }
            if cfg!(target_os = "macos") {
                return reverse_name(address);
            }
            canonical
        }
        Err(_) => name,
    }
}

/// `socket.gethostname()`.
fn hostname() -> String {
    match gethostname::gethostname().into_string() {
        Ok(name) => name,
        // Python decodes the bytes with the file-system encoding and
        // `surrogateescape`, so it never fails; lossy is the closest safe
        // answer.
        Err(name) => name.to_string_lossy().into_owned(),
    }
}

/// The address `setipaddr` picks: the first `getaddrinfo` answer for a name,
/// with the zeroed hints `getaddrinfo(name, NULL, &hints)` uses.
fn first_address(name: &str) -> Option<IpAddr> {
    let answers = dns_lookup::getaddrinfo(Some(name), None, Some(AddrInfoHints::default())).ok()?;
    answers
        .filter_map(Result::ok)
        .next()
        .map(|answer| answer.sockaddr.ip())
}

/// The reverse-mapping name of an address (`1.0.0.127.in-addr.arpa`,
/// `1.0.…ip6.arpa`), which libinfo reports as an alias of every reverse
/// lookup.
fn reverse_name(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => {
            let [a, b, c, d] = address.octets();
            format!("{d}.{c}.{b}.{a}.in-addr.arpa")
        }
        IpAddr::V6(address) => {
            let nibbles = format!("{:032x}", u128::from(address));
            let mut name: String = nibbles
                .chars()
                .rev()
                .flat_map(|nibble| [nibble, '.'])
                .collect();
            name.push_str("ip6.arpa");
            name
        }
    }
}

/// Send one message through Django's SMTP backend semantics.
///
/// The connection is opened per call and closed with `QUIT`, which is what
/// `EmailBackend.send_messages` does for a connection the caller did not
/// already have open (`smtp.py:143-160`).
pub async fn send(configuration: &EmailConfiguration, message: &Message) -> Result<(), MailError> {
    send_with_timeout(configuration, message, None).await
}

/// [`send`] with an explicit socket timeout.
///
/// `manage.py test_email` builds its connection with `timeout=30`
/// (`test_email.py:37-45`); every other Plane call site leaves
/// `EMAIL_TIMEOUT` unset, which is `None`. The timeout bounds the TCP connect
/// here; Django's `smtplib` applies it to every socket operation, a difference
/// that only shows on a server that accepts the connection and then stalls.
pub async fn send_with_timeout(
    configuration: &EmailConfiguration,
    message: &Message,
    timeout: Option<std::time::Duration>,
) -> Result<(), MailError> {
    // `EmailBackend.__init__`: TLS and SSL are mutually exclusive
    // (`smtp.py:47-51`). The endpoint builds the connection outside its `try`,
    // so Django answers 500; the handler maps this to a 500.
    if configuration.use_tls && configuration.use_ssl {
        return Err(MailError::Generic(
            "EMAIL_USE_TLS/EMAIL_USE_SSL are mutually exclusive, so only set one of those \
             settings to True."
                .to_string(),
        ));
    }
    let Some(host) = configuration
        .host
        .as_deref()
        .filter(|host| !host.is_empty())
    else {
        // `smtplib.SMTP(None, port)` never connects and the first command hits
        // `self.sock is None` -> `AttributeError`, which the endpoint's generic
        // branch answers.
        return Err(MailError::Generic(
            "EMAIL_HOST is not configured; smtplib has no socket".to_string(),
        ));
    };
    if configuration.port > u32::from(u16::MAX) {
        // `socket.create_connection` raises OverflowError, caught by the
        // endpoint's generic `except Exception`.
        return Err(MailError::Generic(format!(
            "port must be 0-65535, got {}",
            configuration.port
        )));
    }
    // `DNS_NAME.get_fqdn()`, once per process, for the EHLO argument and the
    // Message-ID domain.
    let local_hostname = dns_name_async().await;
    let mut session = Session::connect(configuration, host, local_hostname, timeout).await?;
    if configuration.use_tls {
        // `starttls()`: a plaintext connection that must upgrade.
        session.starttls(tls_parameters(host)?).await?;
    }
    // `if self.username and self.password:` — an empty password disables
    // authentication entirely (`smtp.py:93-95`).
    let user = configuration.user.as_deref().unwrap_or_default();
    let password = configuration.password.as_deref().unwrap_or_default();
    if !user.is_empty() && !password.is_empty() {
        session.login(user, password).await?;
    }
    // `_send`: the addresses are sanitized and the bytes built after the
    // connection is open, so a `BadHeaderError` here still has a `QUIT`
    // (`send_messages`' `finally`).
    let result = async {
        // `_send` returns False before sanitizing when there is nobody to send
        // to, but `send_messages` has already opened the connection, so the
        // `finally` still quits it (`django/core/mail/backends/smtp.py:139-160`).
        if message.recipients().is_empty() {
            return Ok(());
        }
        let (from, recipients) = message.envelope()?;
        let bytes = message.as_bytes(&default_date(), &default_message_id(local_hostname))?;
        session.transact(&from, &recipients, &bytes).await
    }
    .await;
    // `EmailBackend.send_messages`'s `finally: self.close()` -> `quit()`.
    session.quit().await;
    result
}

// ---------------------------------------------------------------------------
// `smtplib.SMTP`: a command-by-command session
// ---------------------------------------------------------------------------

/// `smtplib._MAXLINE` (`smtplib.py:57`).
const MAX_LINE: usize = 8192;

/// `smtplib._MAXCHALLENGE`.
const MAX_CHALLENGE: u8 = 5;

/// `(code, message)` from `smtplib.getreply` (`smtplib.py:412-460`).
struct Reply {
    /// `int(line[:3])`, or `-1` for a code smtplib cannot parse.
    code: i32,
    /// The reply lines after the code, stripped and joined with `\n`.
    message: String,
}

impl Reply {
    /// `200 <= code <= 299`.
    fn is_positive_completion(&self) -> bool {
        (200..300).contains(&self.code)
    }
}

/// One SMTP session, following `smtplib.SMTP`'s command methods.
struct Session {
    stream: BufReader<AsyncNetworkStream>,
    /// `esmtp_features`: lower-case keyword and its parameters.
    features: Vec<(String, String)>,
    /// `does_esmtp`: EHLO answered 250.
    does_esmtp: bool,
    /// `ehlo_resp`/`helo_resp` were set (`ehlo_or_helo_if_needed`).
    greeted: bool,
    local_hostname: String,
    /// The socket is gone or `smtplib` closed it: no `QUIT` can be sent.
    closed: bool,
}

impl Session {
    /// `SMTP.__init__` + `connect()`: the socket (implicit TLS included), then
    /// the greeting. A greeting other than 220 is `SMTPConnectError`.
    async fn connect(
        configuration: &EmailConfiguration,
        host: &str,
        local_hostname: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<Self, MailError> {
        let port = u16::try_from(configuration.port).map_err(|_| {
            MailError::Generic(format!("port must be 0-65535, got {}", configuration.port))
        })?;
        let tls = if configuration.use_ssl {
            // `smtplib.SMTP_SSL`: TLS from the first byte.
            Some(tls_parameters(host)?)
        } else {
            None
        };
        // `EMAIL_TIMEOUT` is unset in Plane: `None` waits forever. The one
        // exception is `manage.py test_email`, whose `get_connection(timeout=30)`
        // passes a duration through `send_with_timeout`.
        let stream = AsyncNetworkStream::connect_tokio1((host, port), timeout, tls, None)
            .await
            .map_err(map_connect_error)?;
        let mut session = Self {
            stream: BufReader::new(stream),
            features: Vec::new(),
            does_esmtp: false,
            greeted: false,
            local_hostname: local_hostname.to_string(),
            closed: false,
        };
        let reply = session.read_reply().await?;
        if reply.code != 220 {
            // `SMTP.__init__`: `self.close()` then `SMTPConnectError`.
            session.closed = true;
            return Err(MailError::Connect);
        }
        Ok(session)
    }

    /// `ehlo_or_helo_if_needed` (`smtplib.py:495-512`).
    async fn ehlo_or_helo_if_needed(&mut self) -> Result<(), MailError> {
        if self.greeted {
            return Ok(());
        }
        let local_hostname = self.local_hostname.clone();
        let reply = self.command("ehlo", &local_hostname).await?;
        if reply.code == -1 && reply.message.is_empty() {
            // `ehlo()`: a server that disconnects instead of answering.
            self.closed = true;
            return Err(MailError::ServerDisconnected);
        }
        self.apply_ehlo(&reply);
        if !reply.is_positive_completion() {
            let helo = self.command("helo", &local_hostname).await?;
            if !helo.is_positive_completion() {
                // `SMTPHeloError`, which the endpoint's generic branch catches.
                return Err(MailError::Generic(format!(
                    "SMTPHeloError: ({}, {})",
                    helo.code, helo.message
                )));
            }
        }
        self.greeted = true;
        Ok(())
    }

    /// `ehlo()`'s feature parsing (`smtplib.py:465-490`).
    fn apply_ehlo(&mut self, reply: &Reply) {
        self.features.clear();
        self.does_esmtp = false;
        if reply.code != 250 {
            return;
        }
        self.does_esmtp = true;
        // `del resp[0]`: the first line is the server's greeting text.
        for line in reply.message.split('\n').skip(1) {
            // `OLDSTYLE_AUTH = re.compile(r"auth=(.*)", re.I)`.
            if line
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("auth="))
            {
                self.store_feature("auth", line);
                continue;
            }
            if let Some((feature, parameters)) = parse_feature_line(line) {
                self.store_feature(&feature, &parameters);
            }
        }
    }

    /// `esmtp_features[feature] = parameters`, with the `auth` case appending.
    fn store_feature(&mut self, feature: &str, parameters: &str) {
        match self.features.iter_mut().find(|(name, _)| name == feature) {
            Some((_, value)) if feature == "auth" => {
                value.push(' ');
                value.push_str(parameters);
            }
            Some((_, value)) => *value = parameters.to_string(),
            None if feature == "auth" => {
                self.features
                    .push((feature.to_string(), format!(" {parameters}")));
            }
            None => self
                .features
                .push((feature.to_string(), parameters.to_string())),
        }
    }

    fn has_extn(&self, option: &str) -> bool {
        self.features.iter().any(|(name, _)| name == option)
    }

    fn feature(&self, name: &str) -> Option<&str> {
        self.features
            .iter()
            .find(|(feature, _)| feature == name)
            .map(|(_, value)| value.as_str())
    }

    /// `starttls()` (`smtplib.py:775-800`): EHLO, the `STARTTLS` command, the
    /// handshake, then a fresh EHLO.
    async fn starttls(&mut self, tls: TlsParameters) -> Result<(), MailError> {
        self.ehlo_or_helo_if_needed().await?;
        if !self.has_extn("starttls") {
            // `SMTPNotSupportedError`, a generic 400 at the endpoint.
            return Err(MailError::Generic(
                "SMTPNotSupportedError: STARTTLS extension not supported by server.".to_string(),
            ));
        }
        let reply = self.command("STARTTLS", "").await?;
        if reply.code != 220 {
            return Err(MailError::Generic(format!(
                "SMTPResponseException: ({}, {})",
                reply.code, reply.message
            )));
        }
        let upgraded = self.stream.get_mut().upgrade_tls(tls).await;
        if let Err(error) = upgraded {
            self.closed = true;
            return Err(MailError::Generic(format!("TLS error: {error}")));
        }
        // RFC 3207: everything learnt over the plaintext connection is void.
        self.features.clear();
        self.does_esmtp = false;
        self.greeted = false;
        Ok(())
    }

    /// `login()` (`smtplib.py:725-760`): the advertised mechanisms in
    /// `smtplib`'s preference order, continuing after an authentication
    /// failure until one succeeds or the last one fails.
    async fn login(&mut self, user: &str, password: &str) -> Result<(), MailError> {
        self.ehlo_or_helo_if_needed().await?;
        let Some(advertised) = self.feature("auth").map(str::to_string) else {
            // `SMTPNotSupportedError("SMTP AUTH extension not supported by server.")`.
            return Err(MailError::Generic(
                "SMTPNotSupportedError: SMTP AUTH extension not supported by server.".to_string(),
            ));
        };
        let advertised: Vec<&str> = advertised.split_whitespace().collect();
        // CRAM-MD5 needs an HMAC the port does not implement (D23): it is
        // skipped, so a server that only advertises it is a generic 400.
        let mechanisms: Vec<&str> = ["PLAIN", "LOGIN"]
            .into_iter()
            .filter(|mechanism| advertised.contains(mechanism))
            .collect();
        if mechanisms.is_empty() {
            return Err(MailError::Generic(
                "SMTPException: No suitable authentication method found.".to_string(),
            ));
        }
        let mut last = MailError::Authentication;
        for mechanism in mechanisms {
            match self.auth(mechanism, user, password).await {
                Ok(()) => return Ok(()),
                // `except SMTPAuthenticationError as e: last_exception = e`.
                Err(error @ MailError::Authentication) => last = error,
                // A disconnect or a client error is not retried by smtplib.
                Err(error) => return Err(error),
            }
        }
        Err(last)
    }

    /// `auth()` (`smtplib.py:620-680`) for `PLAIN` and `LOGIN`.
    async fn auth(&mut self, mechanism: &str, user: &str, password: &str) -> Result<(), MailError> {
        let initial = match mechanism {
            "PLAIN" => format!("\0{user}\0{password}"),
            // `auth_login(None)` returns the user: LOGIN does send an initial
            // response in smtplib.
            "LOGIN" => user.to_string(),
            other => {
                return Err(MailError::Generic(format!(
                    "SMTPException: unsupported authentication mechanism {other}"
                )));
            }
        };
        let mut count: u8 = 1;
        let mut reply = self
            .raw_command(&format!(
                "AUTH {mechanism} {}",
                base64::engine::general_purpose::STANDARD.encode(initial)
            ))
            .await?;
        while reply.code == 334 {
            count += 1;
            if count > MAX_CHALLENGE {
                return Err(MailError::Generic(
                    "SMTPException: Server AUTH mechanism infinite loop.".to_string(),
                ));
            }
            let response = match mechanism {
                "PLAIN" => format!("\0{user}\0{password}"),
                // `_auth_challenge_count < 2` is only the initial response.
                _ => password.to_string(),
            };
            reply = self
                .raw_command(&base64::engine::general_purpose::STANDARD.encode(response))
                .await?;
        }
        if reply.code == 235 || reply.code == 503 {
            Ok(())
        } else {
            Err(MailError::Authentication)
        }
    }

    /// `sendmail()` (`smtplib.py:860-910`) for one message.
    async fn transact(
        &mut self,
        from: &str,
        recipients: &[String],
        message: &[u8],
    ) -> Result<(), MailError> {
        // `sendmail` starts with `self.ehlo_or_helo_if_needed()`.
        self.ehlo_or_helo_if_needed().await?;
        // `if self.does_esmtp: if self.has_extn("size"): esmtp_opts.append(...)`.
        let mut options = String::new();
        if self.does_esmtp && self.has_extn("size") {
            options = format!(" size={}", message.len());
        }
        let reply = self
            .command("mail", &format!("FROM:{from}{options}"))
            .await?;
        if reply.code != 250 {
            if reply.code == 421 {
                // `self.close()`, not `_rset()`: no QUIT afterwards either.
                self.closed = true;
            } else {
                self.rset().await;
            }
            return Err(MailError::SenderRefused);
        }
        // `senderrs` is a dict keyed by address, so duplicates count once when
        // `len(senderrs) == len(to_addrs)` decides that every recipient was
        // refused.
        let mut refused: Vec<&str> = Vec::new();
        for recipient in recipients {
            let reply = self.command("rcpt", &format!("TO:{recipient}")).await?;
            if reply.code != 250 && reply.code != 251 && !refused.contains(&recipient.as_str()) {
                refused.push(recipient);
            }
            if reply.code == 421 {
                self.closed = true;
                return Err(MailError::RecipientsRefused);
            }
        }
        if refused.len() == recipients.len() {
            self.rset().await;
            return Err(MailError::RecipientsRefused);
        }
        let reply = self.command("data", "").await?;
        if reply.code != 354 {
            // `data()` raises before the payload is sent, without `_rset()`.
            return Err(MailError::Generic(format!(
                "SMTPDataError: ({}, {})",
                reply.code, reply.message
            )));
        }
        self.write(&data_payload(message)).await?;
        let reply = self.read_reply().await?;
        if reply.code != 250 {
            if reply.code == 421 {
                self.closed = true;
            } else {
                self.rset().await;
            }
            return Err(MailError::Generic(format!(
                "SMTPDataError: ({}, {})",
                reply.code, reply.message
            )));
        }
        Ok(())
    }

    /// `quit()` (`smtplib.py:1000-1010`); `_close_connection` swallows a
    /// disconnect, and `quit()` never checks the code.
    async fn quit(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self.command("quit", "").await;
    }

    /// `_rset()`: `RSET`, ignoring a disconnect.
    async fn rset(&mut self) {
        if self.closed {
            return;
        }
        let _ = self.command("rset", "").await;
    }

    /// `putcmd()`: `<line>CRLF`, with smtplib's ASCII encoding rule.
    async fn raw_command(&mut self, line: &str) -> Result<Reply, MailError> {
        if !line.is_ascii() {
            // `s.encode(self.command_encoding)` with `command_encoding = "ascii"`.
            return Err(MailError::Generic(
                "UnicodeEncodeError: 'ascii' codec can't encode characters".to_string(),
            ));
        }
        if line.contains(['\r', '\n']) {
            return Err(MailError::Generic(
                "ValueError: command and arguments contain prohibited newline characters"
                    .to_string(),
            ));
        }
        let mut bytes = Vec::with_capacity(line.len() + 2);
        bytes.extend_from_slice(line.as_bytes());
        bytes.extend_from_slice(b"\r\n");
        self.write(&bytes).await?;
        self.read_reply().await
    }

    /// `docmd()`: a verb and its argument (the argument may be empty).
    async fn command(&mut self, verb: &str, args: &str) -> Result<Reply, MailError> {
        let line = if args.is_empty() {
            verb.to_string()
        } else {
            format!("{verb} {args}")
        };
        self.raw_command(&line).await
    }

    /// `send()` (`smtplib.py:350-370`): any I/O error is
    /// `SMTPServerDisconnected`, because `smtplib` catches `OSError` here.
    async fn write(&mut self, bytes: &[u8]) -> Result<(), MailError> {
        let written = {
            let stream = self.stream.get_mut();
            match stream.write_all(bytes).await {
                Ok(()) => stream.flush().await,
                Err(error) => Err(error),
            }
        };
        if written.is_err() {
            self.closed = true;
            return Err(MailError::ServerDisconnected);
        }
        Ok(())
    }

    /// One line, `\n` included; `false` at end-of-file with nothing read. A
    /// partial line at end-of-file is returned as a line, as `readline` does.
    async fn read_line(&mut self, line: &mut Vec<u8>) -> Result<bool, MailError> {
        loop {
            let available = match self.stream.fill_buf().await {
                Ok(available) => available,
                Err(_) => {
                    self.closed = true;
                    return Err(MailError::ServerDisconnected);
                }
            };
            if available.is_empty() {
                return Ok(!line.is_empty());
            }
            let (end, terminated) = match available.iter().position(|byte| *byte == b'\n') {
                Some(position) => (position + 1, true),
                None => (available.len(), false),
            };
            line.extend_from_slice(&available[..end]);
            self.stream.consume_unpin(end);
            if terminated || line.len() > MAX_LINE {
                return Ok(true);
            }
        }
    }

    /// `getreply()` (`smtplib.py:412-460`).
    async fn read_reply(&mut self) -> Result<Reply, MailError> {
        let mut parts: Vec<String> = Vec::new();
        let code = loop {
            let mut line = Vec::new();
            if !self.read_line(&mut line).await? {
                // `getreply`: an empty read closes and raises
                // `SMTPServerDisconnected`.
                self.closed = true;
                return Err(MailError::ServerDisconnected);
            }
            if line.len() > MAX_LINE {
                self.closed = true;
                return Err(MailError::Generic(
                    "SMTPResponseException: (500, b'Line too long.')".to_string(),
                ));
            }
            let text = String::from_utf8_lossy(&line);
            parts.push(
                text.get(4..)
                    .unwrap_or("")
                    .trim_matches([' ', '\t', '\r', '\n'])
                    .to_string(),
            );
            match text.get(0..3).and_then(|value| value.parse::<i32>().ok()) {
                Some(parsed) => {
                    // `if line[3:4] != b"-": break`.
                    if text.as_bytes().get(3) != Some(&b'-') {
                        break parsed;
                    }
                }
                None => break -1,
            }
        };
        Ok(Reply {
            code,
            message: parts.join("\n"),
        })
    }
}

/// `_quote_periods(msg)` plus the CRLF and `.` terminator `data()` adds
/// (`smtplib.py:600-620`).
fn data_payload(message: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(message.len() + 8);
    let mut at_line_start = true;
    for &byte in message {
        if at_line_start && byte == b'.' {
            payload.push(b'.');
        }
        payload.push(byte);
        at_line_start = byte == b'\n';
    }
    if !message.ends_with(b"\r\n") {
        payload.extend_from_slice(b"\r\n");
    }
    payload.extend_from_slice(b".\r\n");
    payload
}

/// `(?P<feature>[A-Za-z0-9][A-Za-z0-9\-]*) ?` at the start of an EHLO line.
fn parse_feature_line(line: &str) -> Option<(String, String)> {
    let mut characters = line.char_indices();
    let (_, first) = characters.next()?;
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    let mut end = first.len_utf8();
    for (index, character) in characters {
        if character.is_ascii_alphanumeric() || character == '-' {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    Some((
        line[..end].to_ascii_lowercase(),
        line[end..].trim().to_string(),
    ))
}

fn tls_parameters(host: &str) -> Result<TlsParameters, MailError> {
    TlsParameters::new(host.to_string())
        .map_err(|error| MailError::Generic(format!("TLS setup failed: {error}")))
}

/// Map the connection phase's error onto the endpoint's branches.
///
/// `socket.create_connection` failures that are `ConnectionError` subclasses
/// (`ConnectionRefusedError`, `ConnectionResetError`, `ConnectionAbortedError`,
/// `BrokenPipeError`) reach the view's `except ConnectionError`; a timeout
/// reaches `except TimeoutError`; a name-resolution or routing `OSError` and a
/// TLS handshake failure reach `except Exception`.
fn map_connect_error(error: lettre::transport::smtp::Error) -> MailError {
    if error.is_timeout() {
        return MailError::Timeout;
    }
    if error.is_tls() {
        return MailError::Generic(format!("TLS error: {error}"));
    }
    if let Some(io) = error
        .source()
        .and_then(|source| source.downcast_ref::<std::io::Error>())
    {
        return match io.kind() {
            std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe => MailError::Connection,
            std::io::ErrorKind::TimedOut => MailError::Timeout,
            _ => MailError::Generic(format!("OSError: {io}")),
        };
    }
    MailError::Generic(format!("SMTP transport error: {error}"))
}

// ---------------------------------------------------------------------------
// Headers (`django/core/mail/message.py:56-125`, `email/header.py`,
// `email/_policybase.py:348-379`)
// ---------------------------------------------------------------------------

/// `forbid_multi_line_headers(name, value, encoding)` for one header value.
fn encode_header_value(name: &str, value: &str) -> Result<String, MailError> {
    if value.contains('\n') || value.contains('\r') {
        // `BadHeaderError("Header values can't contain newlines (got %r for
        // header %r)")`; the message text is not on the wire.
        return Err(MailError::BadHeader(name.to_string()));
    }
    if value.is_ascii() {
        // `Header(val).encode()` — the utf-8 charset is irrelevant for ASCII.
        return encode_ascii(value, None);
    }
    if is_address_header(name) {
        // `", ".join(sanitize_address(addr, encoding) for addr in
        // getaddresses((val,)))`.
        let addresses = split_address_list(value);
        let mut parts = Vec::with_capacity(addresses.len());
        for address in addresses {
            parts.push(sanitize_address(&address)?);
        }
        return Ok(parts.join(", "));
    }
    // `Header(val, encoding).encode()`; DEFAULT_CHARSET is utf-8, so the
    // encoded words are `=?utf-8?b?…?=`.
    encode_encoded_words(value, None)
}

/// `EmailMessage._set_list_header_if_not_empty` (`message.py:430-441`):
/// `", ".join(str(v) for v in values)`, skipped when the list is empty.
fn write_list_header(out: &mut Vec<u8>, name: &str, values: &[String]) -> Result<(), MailError> {
    if values.is_empty() {
        return Ok(());
    }
    let joined = values.join(", ");
    write_header(out, name, &joined)
}

/// `Compat32._fold` (`email/_policybase.py:348-379`): `"{name}: " +
/// Header(value, header_name=name).encode(linesep="\n", maxlinelen=78) +
/// "\n"`, written with CRLF.
fn write_header(out: &mut Vec<u8>, name: &str, raw_value: &str) -> Result<(), MailError> {
    let value = encode_header_value(name, raw_value)?;
    let folded = fold_ascii(&value, Some(name))?;
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b": ");
    for (index, line) in folded.split('\n').enumerate() {
        if index > 0 {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(line.as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    Ok(())
}

/// `ADDRESS_HEADERS` (`django/core/mail/message.py:49-59`).
fn is_address_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "from"
            | "sender"
            | "reply-to"
            | "to"
            | "cc"
            | "bcc"
            | "resent-from"
            | "resent-sender"
            | "resent-to"
            | "resent-cc"
            | "resent-bcc"
    )
}

/// `getaddresses((value,))` for the address lists Plane's mail path carries:
/// split on commas that are not inside quotes, angle brackets or parentheses.
fn split_address_list(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quotes = 0i32;
    let mut angle = 0i32;
    let mut parens = 0i32;
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if quotes > 0 => {
                current.push(ch);
                escaped = true;
            }
            '"' => {
                if parens == 0 {
                    quotes += 1;
                }
                current.push(ch);
            }
            '<' if quotes % 2 == 0 => {
                angle += 1;
                current.push(ch);
            }
            '>' if quotes % 2 == 0 => {
                angle -= 1;
                current.push(ch);
            }
            '(' if quotes % 2 == 0 => {
                parens += 1;
                current.push(ch);
            }
            ')' if quotes % 2 == 0 => {
                parens -= 1;
                current.push(ch);
            }
            ',' if quotes % 2 == 0 && angle <= 0 && parens == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

/// `sanitize_address(addr, encoding)` (`django/core/mail/message.py:92-125`)
/// for one mailbox.
pub fn sanitize_address(addr: &str) -> Result<String, MailError> {
    let invalid = || MailError::Generic(format!("Invalid address {addr:?}"));
    let (name, address) = parse_mailbox(addr).ok_or_else(invalid)?;
    if !address.contains('@') {
        return Err(invalid());
    }
    if name.contains('\n')
        || name.contains('\r')
        || address.contains('\n')
        || address.contains('\r')
    {
        return Err(MailError::Generic(
            "Invalid address; address parts cannot contain newlines.".to_string(),
        ));
    }
    // `nm.encode("ascii")` -> `Header(nm).encode()`; otherwise
    // `Header(nm, encoding).encode()`.
    let name = if name.is_ascii() {
        if name.is_empty() {
            String::new()
        } else {
            encode_ascii(&name, None)?
        }
    } else {
        encode_encoded_words(&name, None)?
    };
    // `formataddr((nm, parsed_address.addr_spec))`.
    Ok(formataddr(&name, &address))
}

/// `email.headerregistry.get_mailbox` for the mailbox shapes Plane uses:
/// `addr`, `<addr>` and `Name <addr>` (the display name may be quoted).
fn parse_mailbox(addr: &str) -> Option<(String, String)> {
    let trimmed = addr.trim();
    if let Some(close) = trimmed.rfind('>')
        && close == trimmed.len() - 1
        && let Some(open) = trimmed[..close].rfind('<')
    {
        let name = trimmed[..open].trim();
        let address = trimmed[open + 1..close].trim().to_string();
        let name = unquote(name);
        return Some((name, address));
    }
    // A bare address; `get_mailbox` also accepts `Name addr` without angle
    // brackets, which Plane never builds by hand.
    Some((String::new(), trimmed.to_string()))
}

/// The unquoting `sanitize_address` performs through
/// `get_mailbox().display_name` (quoted-string escapes removed).
fn unquote(name: &str) -> String {
    let name = name.trim();
    if name.len() >= 2 && name.starts_with('"') && name.ends_with('"') {
        let inner = &name[1..name.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut escaped = false;
        for ch in inner.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        out
    } else {
        name.to_string()
    }
}

/// `email.utils.formataddr` (`email/utils.py:100-151`) with the default
/// charset.
fn formataddr(name: &str, address: &str) -> String {
    if name.is_empty() {
        return address.to_string();
    }
    // `specialsre = re.compile(r'[][\\()<>@,:;".]')`
    let needs_quotes = name.chars().any(|ch| {
        matches!(
            ch,
            '[' | ']' | '\\' | '(' | ')' | '<' | '>' | '@' | ',' | ':' | ';' | '"' | '.'
        )
    }) && !name.starts_with("=?");
    let name = if needs_quotes {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    } else if name.chars().any(|ch| matches!(ch, '\\' | '"')) {
        // `escapesre.sub(r'\\\g<0>', name)`
        name.replace('\\', "\\\\").replace('"', "\\\"")
    } else {
        name.to_string()
    };
    format!("{name} <{address}>")
}

/// smtplib's `quoteaddr` (`smtplib.py:543-553`): `parseaddr(addr)[1]` in angle
/// brackets.
pub fn quoteaddr(sanitized: &str) -> Option<String> {
    let (_, address) = parse_mailbox(sanitized)?;
    if address.is_empty() {
        return None;
    }
    Some(format!("<{address}>"))
}

/// `Header(value, header_name).encode()` for ASCII values: the
/// `_ValueFormatter` folding algorithm (`email/header.py:407-600`).
fn encode_ascii(value: &str, header_name: Option<&str>) -> Result<String, MailError> {
    let mut formatter = Formatter::new(header_name.map_or(0, |name| name.chars().count() + 2));
    feed_ascii(&mut formatter, "", value, true)?;
    formatter.add_transition();
    Ok(formatter.finish("\n"))
}

/// `fold_ascii` for the generator's second pass: the value is always ASCII
/// (encoded words are), so this is [`encode_ascii`] with the header name.
fn fold_ascii(value: &str, header_name: Option<&str>) -> Result<String, MailError> {
    encode_ascii(value, header_name)
}

/// `Header(value, "utf-8").encode()` for a value with non-ASCII characters:
/// RFC 2047 base64 encoded words (`email/header.py:360-405`,
/// `email/charset.py:293-352`).
fn encode_encoded_words(value: &str, header_name: Option<&str>) -> Result<String, MailError> {
    let mut formatter = Formatter::new(header_name.map_or(0, |name| name.chars().count() + 2));
    feed_encoded(&mut formatter, value)?;
    formatter.add_transition();
    Ok(formatter.finish("\n"))
}

/// `Charset('utf-8').header_encode_lines(string, maxlengths)`
/// (`email/charset.py:293-352`) with base64 chrome.
fn header_encode_lines(value: &str, first_max: usize, rest_max: usize) -> Vec<Option<String>> {
    // `extra = len(charset) + RFC2047_CHROME_LEN` = 5 + 7.
    const EXTRA: usize = DEFAULT_CHARSET.len() + 7;
    let mut lines: Vec<Option<String>> = Vec::new();
    let mut current = String::new();
    let mut maxlen = first_max.saturating_sub(EXTRA);
    for ch in value.chars() {
        current.push(ch);
        let length = header_length(current.as_bytes());
        if length > maxlen {
            current.pop();
            if lines.is_empty() && current.is_empty() {
                lines.push(None);
            } else {
                lines.push(Some(encoded_word(&current)));
            }
            current.clear();
            current.push(ch);
            maxlen = rest_max.saturating_sub(EXTRA);
        }
    }
    lines.push(Some(encoded_word(&current)));
    lines
}

/// `base64mime.header_length`: four characters per three octets.
fn header_length(bytes: &[u8]) -> usize {
    let groups = bytes.len() / 3;
    let leftover = bytes.len() % 3;
    groups * 4 + if leftover > 0 { 4 } else { 0 }
}

/// `base64mime.header_encode(bytes, "utf-8")`.
fn encoded_word(value: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    format!("=?{DEFAULT_CHARSET}?b?{encoded}?=")
}

fn feed_ascii(
    formatter: &mut Formatter,
    fws: &str,
    string: &str,
    split_lines: bool,
) -> Result<(), MailError> {
    let lines: Vec<&str> = if split_lines {
        splitlines_py(string)
    } else {
        vec![string]
    };
    if lines.is_empty() {
        ascii_split(formatter, fws, "");
    } else {
        ascii_split(formatter, fws, lines[0]);
    }
    for line in lines.iter().skip(1) {
        formatter.newline();
        let sline = line.trim_start();
        let fws = &line[..line.len() - sline.len()];
        ascii_split(formatter, fws, sline);
    }
    if lines.len() > 1 {
        formatter.newline();
    }
    Ok(())
}

/// `_ValueFormatter.feed`'s encoded-word branch (`email/header.py:437-470`).
fn feed_encoded(formatter: &mut Formatter, value: &str) -> Result<(), MailError> {
    let first_max = formatter.maxlen.saturating_sub(formatter.current.len());
    let rest_max = formatter
        .maxlen
        .saturating_sub(formatter.continuation.len());
    let mut lines = header_encode_lines(value, first_max, rest_max);
    let Some(first) = lines.first().cloned() else {
        return Ok(());
    };
    lines.remove(0);
    if let Some(first) = first {
        formatter.append_chunk("", &first);
    }
    let Some(last) = lines.pop() else {
        return Ok(());
    };
    formatter.newline();
    if let Some(last) = last {
        formatter
            .current
            .push(formatter.continuation.to_string(), last);
    }
    for line in lines.into_iter().flatten() {
        formatter
            .lines
            .push(format!("{}{line}", formatter.continuation));
    }
    Ok(())
}

/// `_ascii_split` (`email/header.py:472-492`): split on runs of FWS.
///
/// `re.split("([ \t]+)", fws + string)` yields alternating text and whitespace
/// runs; `_append_chunk(fws, part)` takes the *whitespace* run as the prefix of
/// the following text, so a leading text run is fed with an empty prefix.
fn ascii_split(formatter: &mut Formatter, fws: &str, string: &str) {
    let combined = format!("{fws}{string}");
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_is_space = false;
    for ch in combined.chars() {
        let is_space = ch == ' ' || ch == '\t';
        if !current.is_empty() && is_space != current_is_space {
            parts.push(std::mem::take(&mut current));
        }
        current_is_space = is_space;
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    let is_space = |part: &String| part.starts_with([' ', '\t']);

    let mut index = 0;
    if parts.first().is_some_and(|part| !is_space(part)) {
        let part = parts[0].clone();
        formatter.append_chunk("", &part);
        index = 1;
    }
    while index + 1 < parts.len() {
        let prefix = parts[index].clone();
        let part = parts[index + 1].clone();
        formatter.append_chunk(&prefix, &part);
        index += 2;
    }
    if index < parts.len() {
        let part = parts[index].clone();
        if is_space(&part) {
            formatter.append_chunk(&part, "");
        } else {
            formatter.append_chunk("", &part);
        }
    }
}

/// Python's `str.splitlines()`: split on the universal line boundaries and
/// drop a trailing empty element.
fn splitlines_py(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        let boundary = matches!(
            ch,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !boundary {
            continue;
        }
        // A `\r\n` pair is one boundary; the `\r` itself is never part of a
        // line (`str.splitlines`).
        if ch == '\r'
            && let Some((_, '\n')) = chars.peek()
        {
            chars.next();
        }
        lines.push(&text[start..index]);
        start = if ch == '\r' && text.as_bytes().get(index + 1) == Some(&b'\n') {
            index + 2
        } else {
            index + ch.len_utf8()
        };
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// `Generator._write_lines` (`email/generator.py:151-165`): split on
/// `\r\n|\r|\n` and join with CRLF. A trailing newline therefore produces a
/// trailing CRLF, and a body without one gets none.
fn write_crlf_lines(out: &mut Vec<u8>, text: &str) {
    if text.is_empty() {
        return;
    }
    let bytes = text.as_bytes();
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                parts.push(&text[start..index]);
                index += if bytes.get(index + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = index;
            }
            b'\n' => {
                parts.push(&text[start..index]);
                index += 1;
                start = index;
            }
            _ => index += 1,
        }
    }
    parts.push(&text[start..]);
    for (position, line) in parts.iter().enumerate() {
        if position > 0 {
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(line.as_bytes());
    }
}

/// `SafeMIMEText.set_payload`'s long-line test: any line with more than 998
/// octets (`django/core/mail/message.py:174-180`).
fn has_long_lines(body: &str) -> bool {
    splitlines_py(body).iter().any(|line| line.len() > 998)
}

/// `SafeMIMEText(body, subtype, "utf-8")`'s transfer-encoding choice
/// (`django/core/mail/message.py:172-183`).
fn encode_text_body(body: &str) -> (&'static str, String) {
    if has_long_lines(body) {
        ("quoted-printable", quopri_body_encode(body))
    } else if body.is_ascii() {
        ("7bit", body.to_string())
    } else {
        ("8bit", body.to_string())
    }
}

/// One `SafeMIMEText` subpart inside a `multipart/alternative`
/// (`django/core/mail/message.py:492-512`).
fn write_text_part(out: &mut Vec<u8>, content: &str, subtype: &str) -> Result<(), MailError> {
    let mut header = Vec::new();
    write_header(
        &mut header,
        "Content-Type",
        &format!("text/{subtype}; charset=\"utf-8\""),
    )?;
    out.extend_from_slice(&header);
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    let (cte, body) = encode_text_body(content);
    out.extend_from_slice(format!("Content-Transfer-Encoding: {cte}\r\n").as_bytes());
    out.extend_from_slice(b"\r\n");
    write_crlf_lines(out, &body);
    Ok(())
}

/// One `SafeMIMEText` attachment (`django/core/mail/message.py:382-426`):
/// the guessed `text/*` subtype with `charset="utf-8"`, Django's 7bit/8bit/QP
/// choice, then the `Content-Disposition` header.
fn write_attachment_part(out: &mut Vec<u8>, attachment: &Attachment) -> Result<(), MailError> {
    write_header(
        out,
        "Content-Type",
        &format!("{}; charset=\"utf-8\"", attachment.mimetype),
    )?;
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    let (cte, body) = encode_text_body(&attachment.content);
    out.extend_from_slice(format!("Content-Transfer-Encoding: {cte}\r\n").as_bytes());
    write_header(
        out,
        "Content-Disposition",
        &format!("attachment; filename=\"{}\"", attachment.filename),
    )?;
    out.extend_from_slice(b"\r\n");
    write_crlf_lines(out, &body);
    Ok(())
}

/// `email.generator.Generator._make_boundary`: `"=" * 15 + "%019d" % token +
/// "=="`, where `token = random.randrange(sys.maxsize)`.
///
/// The token is random in Django too, so no golden comparison can pin it; the
/// `not in text` retry suffix is omitted because a 19-digit token colliding
/// with an e-mail body is not a reachable case.
fn make_boundary() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).unwrap_or_default();
    let token = u64::from_le_bytes(bytes) % ((i64::MAX - 1) as u64 + 1);
    format!("{}{token:019}==", "===============")
}

/// `email.quoprimime.body_encode(body, maxlinelen=76, eol="\n")`
/// (`email/quoprimime.py:148-224`).
fn quopri_body_encode(body: &str) -> String {
    const MAXLINELEN: usize = 76;
    if body.is_empty() {
        return String::new();
    }
    // The map is over octets, not characters: a UTF-8 `ä` is `=C3=A4`.
    let mut quoted = String::with_capacity(body.len());
    for &byte in body.as_bytes() {
        // `_QUOPRI_BODY_ENCODE_MAP` (`email/quoprimime.py`) leaves space, tab
        // and every printable ASCII character alone; a trailing space is
        // quoted by the line-wrapper below.
        if (byte < 32 && !matches!(byte, b'\t' | b'\n' | b'\r')) || byte == b'=' || byte > 126 {
            quoted.push_str(&format!("={byte:02X}"));
        } else {
            quoted.push(byte as char);
        }
    }
    let soft_break = "=\n";
    let maxlinelen1 = MAXLINELEN - 1;
    let mut encoded: Vec<String> = Vec::new();
    for line in splitlines_py(&quoted) {
        let chars: Vec<char> = line.chars().collect();
        let length = chars.len();
        let mut start = 0usize;
        // `laststart = len(line) - 1 - maxlinelen` on Python ints; negative
        // values make the loop body never run.
        let laststart = length as isize - 1 - MAXLINELEN as isize;
        while (start as isize) <= laststart {
            let stop = start + maxlinelen1;
            if chars[stop - 2] == '=' {
                encoded.push(chars[start..stop - 1].iter().collect());
                start = stop - 2;
            } else if chars[stop - 1] == '=' {
                encoded.push(chars[start..stop].iter().collect());
                start = stop - 1;
            } else {
                let mut chunk: String = chars[start..stop].iter().collect();
                chunk.push('=');
                encoded.push(chunk);
                start = stop;
            }
        }
        if let Some(last) = chars.last().copied().filter(|ch| *ch == ' ' || *ch == '\t') {
            // `room = start - laststart` with Python integer arithmetic.
            let room = start as isize - laststart;
            let q = if room >= 3 {
                format!("={:02X}", last as u32)
            } else if room == 2 {
                format!("{last}{soft_break}")
            } else {
                format!("{soft_break}={:02X}", last as u32)
            };
            let mut rest: String = chars[start..length - 1].iter().collect();
            rest.push_str(&q);
            encoded.push(rest);
        } else {
            encoded.push(chars[start..].iter().collect());
        }
    }
    // `if body[-1] in CRLF: append('')`
    let mut result = encoded.join("\n");
    if body.ends_with('\n') || body.ends_with('\r') {
        result.push('\n');
    }
    result
}

/// `email.header._Accumulator` (`email/header.py:525-600`).
struct Accumulator {
    initial: usize,
    parts: Vec<(String, String)>,
}

impl Accumulator {
    fn len(&self) -> usize {
        self.initial
            + self
                .parts
                .iter()
                .map(|(fws, part)| fws.chars().count() + part.chars().count())
                .sum::<usize>()
    }

    fn push(&mut self, fws: String, part: String) {
        self.parts.push((fws, part));
    }

    /// `pop()`: the last (fws, part), or `("", "")` when empty.
    fn pop(&mut self) -> (String, String) {
        self.parts.pop().unwrap_or_default()
    }

    fn pop_from(&mut self, index: usize) -> Vec<(String, String)> {
        self.parts.split_off(index)
    }

    fn reset(&mut self, parts: Vec<(String, String)>) {
        self.parts = parts;
        self.initial = 0;
    }

    fn is_onlyws(&self) -> bool {
        self.initial == 0 && (self.parts.is_empty() || self.render().trim().is_empty())
    }

    fn part_count(&self) -> usize {
        self.parts.len()
    }

    fn render(&self) -> String {
        self.parts
            .iter()
            .map(|(fws, part)| format!("{fws}{part}"))
            .collect()
    }
}

/// `email.header._ValueFormatter` (`email/header.py:407-523`).
struct Formatter {
    maxlen: usize,
    continuation: &'static str,
    splitchars: &'static str,
    lines: Vec<String>,
    current: Accumulator,
}

impl Formatter {
    fn new(initial: usize) -> Self {
        Self {
            maxlen: 78,
            continuation: " ",
            splitchars: ";, \t",
            lines: Vec::new(),
            current: Accumulator {
                initial,
                parts: Vec::new(),
            },
        }
    }

    fn finish(&mut self, linesep: &str) -> String {
        self.newline();
        self.lines.join(linesep)
    }

    /// `newline()` (`email/header.py:432-444`).
    fn newline(&mut self) {
        let end_of_line = self.current.pop();
        if end_of_line != (" ".to_string(), String::new()) {
            self.current.push(end_of_line.0, end_of_line.1);
        }
        if self.current.len() > 0 {
            if self.current.is_onlyws() && !self.lines.is_empty() {
                let tail = self.current.render();
                if let Some(last) = self.lines.last_mut() {
                    last.push_str(&tail);
                }
            } else {
                self.lines.push(self.current.render());
            }
        }
        self.current.reset(Vec::new());
    }

    /// `add_transition()`.
    fn add_transition(&mut self) {
        self.current.push(" ".to_string(), String::new());
    }

    /// `_append_chunk` (`email/header.py:494-523`).
    fn append_chunk(&mut self, fws: &str, string: &str) {
        self.current.push(fws.to_string(), string.to_string());
        if self.current.len() <= self.maxlen {
            return;
        }
        let mut split: Option<usize> = None;
        'outer: for ch in self.splitchars.chars() {
            for index in (1..self.current.part_count()).rev() {
                if ch.is_whitespace() {
                    let fws = &self.current.parts[index].0;
                    if fws.starts_with(ch) {
                        split = Some(index);
                        break 'outer;
                    }
                }
                let previous = &self.current.parts[index - 1].1;
                if previous.ends_with(ch) {
                    split = Some(index);
                    break 'outer;
                }
            }
        }
        match split {
            Some(index) => {
                let remainder = self.current.pop_from(index);
                self.lines.push(self.current.render());
                self.current.reset(remainder);
            }
            None => {
                let (fws, part) = self.current.pop();
                if self.current.initial > 0 {
                    self.newline();
                    let fws = if fws.is_empty() { " ".to_string() } else { fws };
                    self.current.push(fws, part);
                } else {
                    self.current.push(fws, part);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitlines_matches_python() {
        assert!(splitlines_py("").is_empty());
        assert_eq!(splitlines_py("a\nb"), vec!["a", "b"]);
        assert_eq!(splitlines_py("a\r\nb\r\n"), vec!["a", "b"]);
        assert_eq!(splitlines_py("a\rb"), vec!["a", "b"]);
        assert_eq!(splitlines_py("a\n"), vec!["a"]);
        assert_eq!(splitlines_py("\n\na"), vec!["", "", "a"]);
    }

    #[test]
    fn data_payload_quotes_periods_and_terminates() {
        // `_quote_periods` only at line starts, then the CRLF and dot the DATA
        // terminator needs.
        assert_eq!(data_payload(b"a\r\n.b\r\n"), b"a\r\n..b\r\n.\r\n");
        assert_eq!(data_payload(b"Subject: x"), b"Subject: x\r\n.\r\n");
        assert_eq!(data_payload(b".solo"), b"..solo\r\n.\r\n");
    }

    #[test]
    fn py_int_matches_python() {
        assert_eq!(py_int(Some("587")), Ok(587));
        assert_eq!(py_int(Some(" 587 ")), Ok(587));
        assert_eq!(py_int(Some("+587")), Ok(587));
        assert_eq!(py_int(Some("1_000")), Ok(1000));
        assert!(py_int(Some("587.0")).is_err());
        assert!(py_int(Some("")).is_err());
        assert!(py_int(Some("_1")).is_err());
        assert!(py_int(Some("1_")).is_err());
        assert!(py_int(None).is_err());
        assert!(py_int(Some("-1")).is_err());
    }

    #[test]
    fn envelope_matches_django() {
        let message = Message {
            subject: "S".to_string(),
            body: "B".to_string(),
            from_email: "Team Plane <team@mailer.plane.so>".to_string(),
            to: vec!["receiver@example.com".to_string()],
            cc: Vec::new(),
            reply_to: Vec::new(),
            alternatives: Vec::new(),
            attachments: Vec::new(),
        };
        let (from, recipients) = message.envelope().expect("envelope");
        assert_eq!(from, "<team@mailer.plane.so>");
        assert_eq!(recipients, vec!["<receiver@example.com>"]);
    }

    #[test]
    fn sanitize_address_matches_the_corpus() {
        // `tools/mail/email_message_corpus.py`'s `envelopes` section.
        assert_eq!(
            sanitize_address("Grüße <hello@plane.so>").expect("sanitized"),
            "=?utf-8?b?R3LDvMOfZQ==?= <hello@plane.so>"
        );
        assert_eq!(
            sanitize_address("\"Doe, Jane\" <jane@example.com>").expect("sanitized"),
            "\"Doe, Jane\" <jane@example.com>"
        );
        assert_eq!(
            sanitize_address("Plane <hello@plane.so>").expect("sanitized"),
            "Plane <hello@plane.so>"
        );
        assert!(sanitize_address("").is_err());
    }
}
