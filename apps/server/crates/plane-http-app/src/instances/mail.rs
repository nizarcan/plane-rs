// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! SMTP sending for `/api/instances/email-credentials-check/` (P0b, §2c).
//!
//! Python: `plane/license/api/views/configuration.py:80-171`
//! (`EmailCredentialCheckEndpoint.post`). The view reads seven values with
//! `get_email_configuration` (`plane/license/utils/instance_value.py:42-59`),
//! builds a connection with `django.core.mail.get_connection` and sends one
//! `EmailMultiAlternatives` through `django.core.mail.backends.smtp.EmailBackend`
//! (`django/core/mail/backends/smtp.py:70-186`).
//!
//! Nothing here is registered as a route: set C of `PUBLIC_INSTANCES.md` owns
//! the handler. This module is the transport it calls, so it is `dead_code`
//! until then.
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
//! * The EHLO argument. Django sends `socket.getfqdn()`
//!   (`smtp.py:86-87`); the port lets lettre send its hostname, because
//!   reproducing `gethostbyaddr(gethostname())` needs libc (forbidden). The
//!   value is client metadata, not message content, and no SMTP server rejects
//!   a sync reply for it.
//! * TLS trust. Django builds `ssl.create_default_context()` (the system
//!   store); the port uses `webpki-roots` (DECISIONS.md D9's equivalent of
//!   `certifi`, with `aws-lc-rs` as the single crypto provider).
//! * Authentication mechanism. `smtplib.login` prefers `CRAM-MD5`, then
//!   `PLAIN`, then `LOGIN` (`smtplib.py:725-750`); lettre has no CRAM-MD5, so
//!   the port offers `PLAIN` then `LOGIN`. The outcome (and the failure code
//!   for a wrong password) is the same.
//! * Sender vs recipient refusal. smtplib knows which command failed and raises
//!   `SMTPSenderRefused` or `SMTPRecipientsRefused`
//!   (`smtplib.py:876-900`); lettre's error carries only the reply code, and
//!   550/553 are used for both. Those two branches therefore collapse into
//!   [`MailError::Generic`] with the endpoint's generic message.
//! * `EMAIL_TIMEOUT` is unset in Plane, so Django waits forever; the port
//!   passes `None` to lettre for the same behaviour.

#![allow(dead_code, reason = "set C (instances routes) consumes this transport")]

use base64::Engine as _;
use chrono::{Datelike, Timelike, Utc};
use deadpool_postgres::Client;
use lettre::address::Envelope;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncTransport, Tokio1Executor};
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

/// One `EmailMultiAlternatives` (`django/core/mail/message.py:454-545`) without
/// alternatives or attachments — the only shape this endpoint creates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub subject: String,
    pub body: String,
    pub from_email: String,
    /// `to`, `cc`, `reply_to` in the order the view sets them.
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub reply_to: Vec<String>,
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
        // `SafeMIMEText(body, "plain", "utf-8")` +
        // `SafeMIMEText.set_payload`'s quoted-printable switch
        // (`django/core/mail/message.py:172-183`).
        let mut body = self.body.clone();
        let cte = if has_long_lines(&body) {
            body = quopri_body_encode(&body);
            "quoted-printable"
        } else if body.is_ascii() {
            "7bit"
        } else {
            "8bit"
        };

        let mut out = Vec::with_capacity(512 + body.len());
        // `MIMEText.__init__` writes its own three headers first.
        out.extend_from_slice(b"Content-Type: text/plain; charset=\"utf-8\"\r\n");
        out.extend_from_slice(b"MIME-Version: 1.0\r\n");
        out.extend_from_slice(format!("Content-Transfer-Encoding: {cte}\r\n").as_bytes());
        // `EmailMessage.message()`: Subject, From, To, Cc, Reply-To, Date,
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
        out.extend_from_slice(b"\r\n");
        // `Generator._write_lines`: the payload's line endings become CRLF and
        // a trailing newline is not doubled.
        write_crlf_lines(&mut out, &body);
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
/// The domain is the process's `HOSTNAME` (or `localhost`), where Python uses
/// `socket.getfqdn()`; see the module docs.
#[must_use]
pub fn default_message_id() -> String {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let timeval = since_epoch.as_micros() / 10_000;
    let pid = std::process::id();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).unwrap_or_default();
    let randint = u64::from_le_bytes(random);
    let domain = std::env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "localhost".to_string());
    format!("<{timeval}.{pid}.{randint}@{domain}>")
}

/// Send one message through Django's SMTP backend semantics.
///
/// The connection is opened per call and closed with `QUIT`, which is what
/// `EmailBackend.send_messages` does for a connection the caller did not
/// already have open (`smtp.py:143-160`).
pub async fn send(configuration: &EmailConfiguration, message: &Message) -> Result<(), MailError> {
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
    // `EmailMessage.send` does not connect when there is nobody to send to
    // (`django/core/mail/message.py:334-340`).
    let (from, recipients) = message.envelope()?;
    if recipients.is_empty() {
        return Ok(());
    }

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
        .port(configuration.port as u16)
        // `EMAIL_TIMEOUT` is unset in Plane, i.e. no timeout.
        .timeout(None);
    builder = if configuration.use_ssl {
        // `smtplib.SMTP_SSL`: TLS from the first byte.
        builder.tls(Tls::Wrapper(tls_parameters(host)?))
    } else if configuration.use_tls {
        // `starttls()`: a plaintext connection that must upgrade.
        builder.tls(Tls::Required(tls_parameters(host)?))
    } else {
        builder.tls(Tls::None)
    };
    // `if self.username and self.password:` — an empty password disables
    // authentication entirely (`smtp.py:93-95`).
    let user = configuration.user.as_deref().unwrap_or_default();
    let password = configuration.password.as_deref().unwrap_or_default();
    if !user.is_empty() && !password.is_empty() {
        builder = builder
            .credentials(Credentials::new(user.to_string(), password.to_string()))
            .authentication(vec![Mechanism::Plain, Mechanism::Login]);
    }
    let transport = builder.build();

    // `envelope()` returns smtplib's `<addr-spec>` form; lettre wants the bare
    // addr-spec.
    let from = lettre::Address::try_from(strip_brackets(&from).to_string())
        .map_err(|error| MailError::Generic(format!("invalid From address: {error}")))?;
    let mut to = Vec::with_capacity(recipients.len());
    for recipient in &recipients {
        to.push(
            lettre::Address::try_from(strip_brackets(recipient).to_string())
                .map_err(|error| MailError::Generic(format!("invalid recipient: {error}")))?,
        );
    }
    let envelope = Envelope::new(Some(from), to)
        .map_err(|error| MailError::Generic(format!("invalid envelope: {error}")))?;
    let bytes = message.as_bytes(&default_date(), &default_message_id())?;
    match transport.send_raw(&envelope, &bytes).await {
        Ok(_response) => Ok(()),
        Err(error) => Err(map_error(error)),
    }
}

/// `<a@b>` -> `a@b`.
fn strip_brackets(address: &str) -> &str {
    address
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(address)
}

fn tls_parameters(host: &str) -> Result<TlsParameters, MailError> {
    TlsParameters::new(host.to_string())
        .map_err(|error| MailError::Generic(format!("TLS setup failed: {error}")))
}

/// Map lettre's error onto the endpoint's branches.
///
/// lettre reports the reply code but not the command that produced it, so the
/// two refusals that share codes (sender and recipient) are not separable; see
/// the module docs.
fn map_error(error: lettre::transport::smtp::Error) -> MailError {
    if error.is_timeout() {
        return MailError::Timeout;
    }
    if let Some(status) = error.status() {
        let code = u16::from(status);
        return match code {
            // `smtplib.login` raises SMTPAuthenticationError for anything but
            // 235/503.
            530 | 534 | 535 | 538 => MailError::Authentication,
            // 421: the server is closing the transmission channel.
            421 => MailError::ServerDisconnected,
            // 550/551/553 are `SMTPSenderRefused` *and*
            // `SMTPRecipientsRefused`; the endpoint's generic branch is the
            // only honest answer. A 554 is the greeting refusal (SMTPConnectError)
            // only when it arrives before EHLO, which lettre does not report.
            550 | 551 | 553 | 554 => MailError::Generic(format!("SMTP reply {code}")),
            _ => MailError::Generic(format!("SMTP reply {code}")),
        };
    }
    if error.is_tls() {
        return MailError::Generic(format!("TLS error: {error}"));
    }
    if error.is_response() {
        // lettre parses every reply; a reply it cannot parse is an incomplete
        // one, i.e. the server closed the connection mid-response. smtplib
        // raises `SMTPServerDisconnected` for that (`code == -1`).
        return MailError::ServerDisconnected;
    }
    if let Some(source) = std::error::Error::source(&error)
        && let Some(io) = source.downcast_ref::<std::io::Error>()
    {
        return match io.kind() {
            std::io::ErrorKind::ConnectionRefused => MailError::Connection,
            std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::NotConnected => MailError::ServerDisconnected,
            _ => MailError::Generic(format!("SMTP transport error: {error}")),
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
        if (byte < 33 && !matches!(byte, b'\t' | b'\n' | b'\r')) || byte == b'=' || byte > 126 {
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
