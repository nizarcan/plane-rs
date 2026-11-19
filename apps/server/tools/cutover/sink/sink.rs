// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The exit check's local SMTP sink, webhook listener and fake GitHub.
//!
//! `tools/cutover/exit_check.py` must prove the cutover stack sends e-mail and
//! webhooks without reaching the internet and without a Python process in any
//! container (`plans/CUTOVER.md` §5). This binary is one non-Python container
//! that offers all three:
//!
//! * SMTP on `SMTP_PORT` (25 by default): accept every message, record the
//!   envelope and the raw bytes under `LOG_DIR`;
//! * HTTP on `HTTP_PORT` (8080): record every webhook request under `LOG_DIR`
//!   and answer 200; a `/repos/...` path answers GitHub's `releases/latest`
//!   JSON, which `PLANE_RS_GITHUB_RELEASES_URL` points at.
//!
//! Deliberately std-only: it is compiled in the throwaway `cutover-sink` image
//! (`sink/Dockerfile`), is never part of the workspace, and is not a
//! deployment component.

use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

fn main() {
    let log_dir = PathBuf::from(env::var("LOG_DIR").unwrap_or_else(|_| "/logs".to_string()));
    fs::create_dir_all(&log_dir).expect("creating LOG_DIR");
    let smtp_port: u16 = env::var("SMTP_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(25);
    let http_port: u16 = env::var("HTTP_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);

    let counter = Arc::new(Counter::default());
    println!("cutover-sink: SMTP :{smtp_port}, HTTP :{http_port}, logs {log_dir:?}");
    let _ = std::io::stdout().flush();

    let smtp = {
        let log_dir = log_dir.clone();
        let counter = Arc::clone(&counter);
        thread::spawn(move || {
            serve(TcpListener::bind(("0.0.0.0", smtp_port)), move |stream| {
                smtp_connection(stream, &log_dir, &counter)
            })
        })
    };
    let http = {
        let log_dir = log_dir.clone();
        let counter = Arc::clone(&counter);
        thread::spawn(move || {
            serve(TcpListener::bind(("0.0.0.0", http_port)), move |stream| {
                http_connection(stream, &log_dir, &counter)
            })
        })
    };
    let _ = smtp.join();
    let _ = http.join();
}

#[derive(Default)]
struct Counter {
    smtp: AtomicU64,
    http: AtomicU64,
}

fn serve<F>(listener: std::io::Result<TcpListener>, handler: F)
where
    F: Fn(TcpStream) + Send + Sync + 'static,
{
    let listener = listener.expect("binding the sink listener");
    let handler = Arc::new(handler);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let handler = Arc::clone(&handler);
                thread::spawn(move || handler(stream));
            }
            Err(error) => eprintln!("cutover-sink: accept failed: {error}"),
        }
    }
}

fn smtp_connection(stream: TcpStream, log_dir: &PathBuf, counter: &Counter) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(60)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;
    let _ = writer.write_all(b"220 cutover-sink ESMTP\r\n");
    let mut mail_from = String::new();
    let mut rcpt_to: Vec<String> = Vec::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = line.trim_end_matches(['\r', '\n']).to_string();
        let upper = command.to_ascii_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            let _ = writer.write_all(b"250-cutover-sink\r\n250-SIZE 10485760\r\n250 8BITMIME\r\n");
        } else if upper.starts_with("MAIL FROM:") {
            mail_from = command[10..].trim().to_string();
            let _ = writer.write_all(b"250 OK\r\n");
        } else if upper.starts_with("RCPT TO:") {
            rcpt_to.push(command[8..].trim().to_string());
            let _ = writer.write_all(b"250 OK\r\n");
        } else if upper.starts_with("DATA") {
            let _ = writer.write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n");
            let mut message = Vec::new();
            loop {
                let mut data_line = String::new();
                match reader.read_line(&mut data_line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if data_line == ".\r\n" || data_line == ".\n" {
                    break;
                }
                let data_line = data_line.strip_prefix('.').unwrap_or(&data_line);
                message.extend_from_slice(data_line.as_bytes());
            }
            let number = counter.smtp.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = fs::write(log_dir.join(format!("smtp-{number}.eml")), &message);
            let envelope = format!(
                "mail_from: {mail_from}\nrcpt_to: {}\nsize: {}\n",
                rcpt_to.join(", "),
                message.len()
            );
            let _ = fs::write(log_dir.join(format!("smtp-{number}.envelope")), envelope);
            let _ = writer.write_all(b"250 OK\r\n");
        } else if upper.starts_with("QUIT") {
            let _ = writer.write_all(b"221 Bye\r\n");
            return;
        } else if upper.starts_with("RSET") || upper.starts_with("NOOP") {
            let _ = writer.write_all(b"250 OK\r\n");
        } else {
            let _ = writer.write_all(b"500 Command not recognized\r\n");
        }
    }
}

fn http_connection(stream: TcpStream, log_dir: &PathBuf, counter: &Counter) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let Ok(reader_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
        return;
    }
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
            if name.eq_ignore_ascii_case("expect")
                && value.trim().eq_ignore_ascii_case("100-continue")
            {
                let _ = writer.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
            }
        }
        headers.push(line.trim_end_matches(['\r', '\n']).to_string());
    }
    let mut body = vec![0u8; content_length];
    let _ = reader.read_exact(&mut body);

    if path.starts_with("/repos/") {
        let payload = br#"{"tag_name":"v9.9.9","name":"v9.9.9"}"#;
        let _ = writer.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        );
        let _ = writer.write_all(payload);
        return;
    }

    let number = counter.http.fetch_add(1, Ordering::SeqCst) + 1;
    let mut recorded = Vec::new();
    recorded.extend_from_slice(request_line.as_bytes());
    for header in &headers {
        recorded.extend_from_slice(header.as_bytes());
        recorded.push(b'\n');
    }
    recorded.push(b'\n');
    recorded.extend_from_slice(&body);
    let _ = fs::write(log_dir.join(format!("webhook-{number}.http")), recorded);

    let payload = br#"{"ok":true}"#;
    let _ = writer.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    );
    let _ = writer.write_all(payload);
}
