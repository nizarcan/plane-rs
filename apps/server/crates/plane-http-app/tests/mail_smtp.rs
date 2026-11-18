// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The SMTP session: envelope, message bytes on the wire, command order and
//! the error map.
//!
//! `tests/email_message_golden.rs` pins the bytes Django's
//! `EmailMultiAlternatives` produces; this test proves they are what the
//! transport actually transmits, with the same `MAIL FROM:`/`RCPT TO:`
//! envelope and `smtplib`'s command sequence, and that a fake server's failure
//! modes land in the endpoint's branches
//! (`plane/license/api/views/configuration.py:122-171`).
//!
//! The listener is in-process so the test needs no Python and no open port.

use std::sync::{Arc, Mutex};

use plane_http_app::instances::mail::{
    EmailConfiguration, MailError, Message, dns_name, getfqdn, send,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[derive(Default, Clone)]
struct Recorded {
    commands: Vec<String>,
    mail_from: Option<String>,
    rcpt_to: Vec<String>,
    data: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Behaviour {
    Accept,
    RequireAuth,
    RejectEhlo,
    RejectSender,
    RejectRecipient,
    RejectData,
    Greeting(u16),
    DropAfterGreeting,
    QuitError,
}

const PLAIN_OK: &str = "AHVzZXIAcGxhbmU="; // base64("\0user\0plane")
const LOGIN_USER: &str = "dXNlcg=="; // base64("user")
const LOGIN_PASSWORD: &str = "cGxhbmU="; // base64("plane")

/// `FROM:<a@b> size=42` -> `<a@b>`, like `fake_smtp.py`'s `split_envelope`.
fn address_only(rest: &str) -> String {
    let rest = rest.trim();
    let start = rest.find('<');
    let end = start.and_then(|start| rest[start..].find('>').map(|end| start + end));
    match (start, end) {
        (Some(start), Some(end)) => rest[start..=end].to_string(),
        _ => rest.to_string(),
    }
}

struct FakeSmtp {
    port: u16,
    recorded: Arc<Mutex<Recorded>>,
}

impl FakeSmtp {
    async fn start(behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let shared = Arc::clone(&recorded);
        tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            let greeting = match behaviour {
                Behaviour::Greeting(code) => format!("{code} no service"),
                _ => "220 fake ready".to_string(),
            };
            writer
                .write_all(format!("{greeting}\r\n").as_bytes())
                .await
                .ok();
            if behaviour == Behaviour::DropAfterGreeting {
                return;
            }
            let mut envelope_from: Option<String> = None;
            let mut envelope_to: Vec<String> = Vec::new();
            let mut authenticated = false;
            loop {
                line.clear();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let command = line.trim_end().to_string();
                shared.lock().expect("lock").commands.push(command.clone());
                let upper = command.to_ascii_uppercase();
                if upper.starts_with("EHLO") || upper.starts_with("HELO") {
                    if behaviour == Behaviour::RejectEhlo && upper.starts_with("EHLO") {
                        writer.write_all(b"500 5.5.1 what\r\n").await.ok();
                        continue;
                    }
                    let features = if behaviour == Behaviour::RequireAuth {
                        "250-fake\r\n250-SIZE 10485760\r\n250-AUTH PLAIN LOGIN\r\n250 HELP"
                    } else {
                        "250-fake\r\n250-SIZE 10485760\r\n250 HELP"
                    };
                    writer
                        .write_all(format!("{features}\r\n").as_bytes())
                        .await
                        .ok();
                } else if upper.starts_with("AUTH PLAIN") {
                    // The initial response carries base64("\0user\0plane").
                    let accepted = command.ends_with(PLAIN_OK);
                    authenticated = accepted;
                    writer
                        .write_all(if accepted {
                            b"235 ok\r\n".as_slice()
                        } else {
                            b"535 authentication failed\r\n".as_slice()
                        })
                        .await
                        .ok();
                } else if upper.starts_with("AUTH LOGIN") {
                    // smtplib sends `AUTH LOGIN <base64(user)>` as the initial
                    // response, then the password after a 334 challenge.
                    if command.ends_with(LOGIN_USER) {
                        writer.write_all(b"334 UGFzc3dvcmQ6\r\n").await.ok();
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        let password = line.trim_end().to_string();
                        shared.lock().expect("lock").commands.push(password.clone());
                        authenticated = password == LOGIN_PASSWORD;
                    }
                    writer
                        .write_all(if authenticated {
                            b"235 ok\r\n".as_slice()
                        } else {
                            b"535 authentication failed\r\n".as_slice()
                        })
                        .await
                        .ok();
                } else if upper.starts_with("MAIL FROM:") {
                    if behaviour == Behaviour::RequireAuth && !authenticated {
                        writer.write_all(b"530 auth required\r\n").await.ok();
                        continue;
                    }
                    if behaviour == Behaviour::RejectSender {
                        writer.write_all(b"550 sender refused\r\n").await.ok();
                        continue;
                    }
                    envelope_from = Some(address_only(&command["MAIL FROM:".len()..]));
                    writer.write_all(b"250 ok\r\n").await.ok();
                } else if upper.starts_with("RCPT TO:") {
                    if behaviour == Behaviour::RejectRecipient {
                        writer.write_all(b"550 recipient refused\r\n").await.ok();
                        continue;
                    }
                    envelope_to.push(address_only(&command["RCPT TO:".len()..]));
                    writer.write_all(b"250 ok\r\n").await.ok();
                } else if upper == "DATA" {
                    writer.write_all(b"354 go ahead\r\n").await.ok();
                    let mut data = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                            break;
                        }
                        if line == ".\r\n" || line == "." {
                            break;
                        }
                        data.push_str(line.strip_prefix("..").unwrap_or(&line));
                    }
                    if behaviour == Behaviour::RejectData {
                        writer.write_all(b"554 data refused\r\n").await.ok();
                        continue;
                    }
                    {
                        // The CRLF before the terminating `.` belongs to the
                        // end-of-data sequence, not to the message (RFC 5321
                        // §4.1.1.4): a receiver removes it, so a message that
                        // ends with a newline is indistinguishable from one
                        // that does not.
                        let data = data.strip_suffix("\r\n").unwrap_or(&data).to_string();
                        let mut recorded = shared.lock().expect("lock");
                        recorded.mail_from = envelope_from.take();
                        recorded.rcpt_to = std::mem::take(&mut envelope_to);
                        recorded.data = Some(data);
                    }
                    writer.write_all(b"250 queued\r\n").await.ok();
                } else if upper == "QUIT" {
                    if behaviour == Behaviour::QuitError {
                        writer.write_all(b"554 quit refused\r\n").await.ok();
                    } else {
                        writer.write_all(b"221 bye\r\n").await.ok();
                    }
                    break;
                } else if upper == "RSET" {
                    envelope_from = None;
                    envelope_to.clear();
                    writer.write_all(b"250 ok\r\n").await.ok();
                } else {
                    writer.write_all(b"250 ok\r\n").await.ok();
                }
            }
        });
        Self { port, recorded }
    }

    fn commands(&self) -> Vec<String> {
        self.recorded.lock().expect("lock").commands.clone()
    }
}

fn configuration(port: u16) -> EmailConfiguration {
    EmailConfiguration {
        host: Some("127.0.0.1".to_string()),
        user: None,
        password: None,
        port: u32::from(port),
        use_tls: false,
        use_ssl: false,
        from: "Team Plane <team@mailer.plane.so>".to_string(),
    }
}

fn message() -> Message {
    Message::email_credentials_check("receiver@example.com", "Team Plane <team@mailer.plane.so>")
}

#[tokio::test]
async fn a_successful_send_writes_djangos_bytes_and_envelope() {
    let server = FakeSmtp::start(Behaviour::Accept).await;
    send(&configuration(server.port), &message())
        .await
        .expect("send");
    let recorded = server.recorded.lock().expect("lock").clone();
    assert_eq!(
        recorded.mail_from.as_deref(),
        Some("<team@mailer.plane.so>"),
        "{:?}",
        recorded.commands
    );
    assert_eq!(
        recorded.rcpt_to,
        vec!["<receiver@example.com>".to_string()],
        "{:?}",
        recorded.commands
    );
    let data = recorded.data.expect("data");
    // `Generator._write_lines` ends the headers with CRLF and leaves the body
    // without a trailing newline; the generated Date/Message-ID are between.
    assert!(
        data.starts_with(
            "Content-Type: text/plain; charset=\"utf-8\"\r\nMIME-Version: 1.0\r\n\
             Content-Transfer-Encoding: 7bit\r\nSubject: Email Notification from Plane\r\n\
             From: Team Plane <team@mailer.plane.so>\r\nTo: receiver@example.com\r\nDate: "
        ),
        "{data:?}"
    );
    assert!(
        data.ends_with("\r\n\r\nThis is a sample email notification sent from Plane application."),
        "{data:?}"
    );
    // The same `DNS_NAME` for the EHLO argument and the Message-ID domain.
    let fqdn = dns_name();
    assert!(
        data.contains(&format!("@{fqdn}>")),
        "the Message-ID domain is not DNS_NAME ({fqdn}): {data:?}"
    );
    // smtplib's command sequence: `ehlo`, `mail` with `size=` (the server
    // advertises SIZE), `rcpt`, `data`, then `quit` from
    // `EmailBackend.close` (`smtp.py:143-160`).
    assert_eq!(
        recorded
            .commands
            .iter()
            .map(|command| command.split(' ').next().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["ehlo", "mail", "rcpt", "data", "quit"],
        "{:?}",
        recorded.commands
    );
    assert_eq!(
        recorded.commands[0],
        format!("ehlo {fqdn}"),
        "{:?}",
        recorded.commands
    );
    assert!(
        recorded.commands[1].contains(" size="),
        "{:?}",
        recorded.commands
    );
}

#[tokio::test]
async fn a_wrong_password_tries_every_mechanism_and_is_the_authentication_branch() {
    let server = FakeSmtp::start(Behaviour::RequireAuth).await;
    let config = EmailConfiguration {
        user: Some("user".to_string()),
        password: Some("wrong".to_string()),
        ..configuration(server.port)
    };
    assert_eq!(
        send(&config, &message()).await,
        Err(MailError::Authentication)
    );
    assert_eq!(
        MailError::Authentication.endpoint_message(),
        Some("Invalid credentials provided")
    );
    let commands = server.commands();
    // `smtplib.login` retries the next advertised mechanism after a 535.
    assert_eq!(
        commands
            .iter()
            .filter(|command| command.starts_with("AUTH "))
            .count(),
        2,
        "{commands:?}"
    );
    assert!(commands[1].starts_with("AUTH PLAIN "), "{commands:?}");
    assert!(commands[2].starts_with("AUTH LOGIN "), "{commands:?}");
    // `open()` failed, so `send_messages` never reached its `finally`: no QUIT.
    assert!(
        !commands
            .iter()
            .any(|command| command.eq_ignore_ascii_case("quit")),
        "{commands:?}"
    );
}

#[tokio::test]
async fn a_correct_password_authenticates() {
    let server = FakeSmtp::start(Behaviour::RequireAuth).await;
    let config = EmailConfiguration {
        user: Some("user".to_string()),
        password: Some("plane".to_string()),
        ..configuration(server.port)
    };
    send(&config, &message()).await.expect("send");
    let commands = server.commands();
    assert!(commands[1].starts_with("AUTH PLAIN "), "{commands:?}");
    assert_eq!(
        commands.last().map(String::as_str),
        Some("quit"),
        "{commands:?}"
    );
}

#[tokio::test]
async fn a_closed_port_is_the_connection_branch() {
    // Bind and drop, so the port is (almost certainly) closed.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    assert_eq!(
        send(&configuration(port), &message()).await,
        Err(MailError::Connection)
    );
}

#[tokio::test]
async fn a_greeting_refusal_is_smtp_connect() {
    let server = FakeSmtp::start(Behaviour::Greeting(554)).await;
    assert_eq!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::Connect)
    );
    assert_eq!(
        MailError::Connect.endpoint_message(),
        Some("Could not connect with the SMTP server.")
    );
}

#[tokio::test]
async fn a_dropped_connection_is_server_disconnected() {
    let server = FakeSmtp::start(Behaviour::DropAfterGreeting).await;
    assert_eq!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::ServerDisconnected)
    );
}

#[tokio::test]
async fn a_sender_refusal_is_its_own_branch() {
    let server = FakeSmtp::start(Behaviour::RejectSender).await;
    assert_eq!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::SenderRefused)
    );
    assert_eq!(
        MailError::SenderRefused.endpoint_message(),
        Some("From address is invalid.")
    );
    // `sendmail`'s `_rset()` after the refusal, then `send_messages`' QUIT.
    let commands = server.commands();
    assert!(
        commands[1].starts_with("mail FROM:<team@mailer.plane.so> size="),
        "{commands:?}"
    );
    let verbs: Vec<&str> = commands
        .iter()
        .map(|command| command.split(' ').next().unwrap_or_default())
        .collect();
    assert_eq!(verbs, vec!["ehlo", "mail", "rset", "quit"], "{commands:?}");
}

#[tokio::test]
async fn a_recipient_refusal_is_its_own_branch() {
    let server = FakeSmtp::start(Behaviour::RejectRecipient).await;
    assert_eq!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::RecipientsRefused)
    );
    assert_eq!(
        MailError::RecipientsRefused.endpoint_message(),
        Some("All recipient addresses were refused.")
    );
    let verbs: Vec<String> = server
        .commands()
        .iter()
        .map(|command| command.split(' ').next().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        verbs,
        vec!["ehlo", "mail", "rcpt", "rset", "quit"],
        "{verbs:?}"
    );
}

#[tokio::test]
async fn a_data_refusal_is_the_generic_branch() {
    let server = FakeSmtp::start(Behaviour::RejectData).await;
    assert!(matches!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::Generic(_))
    ));
    let verbs: Vec<String> = server
        .commands()
        .iter()
        .map(|command| command.split(' ').next().unwrap_or_default().to_string())
        .collect();
    // `SMTPDataError` is not one of the endpoint's branches: generic 400, but
    // `sendmail` still reset and `send_messages` still quit.
    assert_eq!(
        verbs,
        vec!["ehlo", "mail", "rcpt", "data", "rset", "quit"],
        "{verbs:?}"
    );
}

#[tokio::test]
async fn a_failed_ehlo_falls_back_to_helo() {
    let server = FakeSmtp::start(Behaviour::RejectEhlo).await;
    send(&configuration(server.port), &message())
        .await
        .expect("send");
    let commands = server.commands();
    assert_eq!(commands[0], format!("ehlo {}", dns_name()), "{commands:?}");
    assert_eq!(commands[1], format!("helo {}", dns_name()), "{commands:?}");
    // `does_esmtp` stays false after a non-250 EHLO, so no `size=` option.
    assert!(!commands[2].contains("size="), "{commands:?}");
}

#[tokio::test]
async fn a_refused_quit_is_still_a_success() {
    let server = FakeSmtp::start(Behaviour::QuitError).await;
    send(&configuration(server.port), &message())
        .await
        .expect("quit() never checks the reply code");
    assert_eq!(server.commands().last().map(String::as_str), Some("quit"));
}

#[test]
fn getfqdn_keeps_the_python_contract() {
    // `socket.getfqdn`'s argument normalisation: surrounding whitespace.
    assert_eq!(getfqdn(" localhost "), getfqdn("localhost"));
    // The cached value is what the transport sends in EHLO and the
    // Message-ID; it is never empty.
    assert!(!dns_name().is_empty());
    assert!(!dns_name().contains(char::is_whitespace));
    // `0.0.0.0` means "the local host", exactly like an empty argument.
    assert_eq!(getfqdn("0.0.0.0"), getfqdn(""));
}
