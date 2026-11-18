// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The lettre wiring: envelope, message bytes on the wire, and the error map.
//!
//! `tests/email_message_golden.rs` pins the bytes Django's
//! `EmailMultiAlternatives` produces; this test proves they are what the
//! transport actually transmits, with the same `MAIL FROM:`/`RCPT TO:`
//! envelope, and that a fake server's failure modes land in the endpoint's
//! branches (`plane/license/api/views/configuration.py:122-171`).
//!
//! The listener is in-process so the test needs no Python and no open port.

use std::sync::{Arc, Mutex};

use plane_http_app::instances::mail::{EmailConfiguration, MailError, Message, send};
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
    RejectSender,
    RejectRecipient,
    Greeting(u16),
    DropAfterGreeting,
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
                    let features = if behaviour == Behaviour::RequireAuth {
                        "250-fake\r\n250-AUTH PLAIN LOGIN\r\n250 HELP"
                    } else {
                        "250-fake\r\n250 HELP"
                    };
                    writer
                        .write_all(format!("{features}\r\n").as_bytes())
                        .await
                        .ok();
                } else if upper.starts_with("AUTH PLAIN") {
                    // The initial response carries base64("\0user\0plane").
                    let accepted = command.ends_with("AHVzZXIAcGxhbmU=");
                    authenticated = accepted;
                    writer
                        .write_all(if accepted {
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
                    envelope_from = Some(command["MAIL FROM:".len()..].trim().to_string());
                    writer.write_all(b"250 ok\r\n").await.ok();
                } else if upper.starts_with("RCPT TO:") {
                    if behaviour == Behaviour::RejectRecipient {
                        writer.write_all(b"550 recipient refused\r\n").await.ok();
                        continue;
                    }
                    envelope_to.push(command["RCPT TO:".len()..].trim().to_string());
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
                    writer.write_all(b"221 bye\r\n").await.ok();
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
    assert_eq!(recorded.rcpt_to, vec!["<receiver@example.com>".to_string()]);
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
    assert!(
        data.contains("\r\nMessage-ID: <") && data.contains("@"),
        "the generated Message-ID header is missing: {data:?}"
    );
    // QUIT, as `EmailBackend.close` does (`smtp.py:143-160`).
    assert_eq!(recorded.commands.last().map(String::as_str), Some("QUIT"));
}

#[tokio::test]
async fn a_wrong_password_is_the_authentication_branch() {
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
async fn a_greeting_refusal_is_not_smtp_connect() {
    // lettre reports the 554 but not the phase; the module docs record why the
    // endpoint's generic branch is the only honest answer here.
    let server = FakeSmtp::start(Behaviour::Greeting(554)).await;
    assert!(matches!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::Generic(_))
    ));
}

#[tokio::test]
async fn a_dropped_connection_is_server_disconnected() {
    let server = FakeSmtp::start(Behaviour::DropAfterGreeting).await;
    assert_eq!(
        send(&configuration(server.port), &message()).await,
        Err(MailError::ServerDisconnected)
    );
}
