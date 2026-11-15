// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Minimal Redis client: the harness only needs `SELECT` + `FLUSHDB` to give
//! each target a clean database index, so it speaks the three RESP types it
//! can receive instead of pulling in a full client.

use anyhow::{Context, Result, anyhow, bail};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RespValue {
    Simple(String),
    Integer(i64),
    Bulk(Option<String>),
    Array(Vec<RespValue>),
}

impl RespValue {
    pub fn as_status(&self) -> Option<&str> {
        match self {
            RespValue::Simple(text) => Some(text),
            _ => None,
        }
    }
}

pub struct RedisAdmin {
    host: String,
    port: u16,
    password: Option<String>,
}

impl RedisAdmin {
    pub fn from_url(url: &str) -> Result<Self> {
        let rest = url
            .split_once("://")
            .map(|(_, rest)| rest)
            .ok_or_else(|| anyhow!("Redis URL {url:?} has no scheme"))?;
        let rest = rest.split('/').next().unwrap_or(rest);
        let (auth, host_port) = match rest.rsplit_once('@') {
            Some((auth, host_port)) => (Some(auth), host_port),
            None => (None, rest),
        };
        let (host, port) = match host_port.rsplit_once(':') {
            Some((host, port)) => (
                host.to_string(),
                port.parse::<u16>()
                    .with_context(|| format!("parsing Redis port {port:?}"))?,
            ),
            None => (host_port.to_string(), 6379),
        };
        if host.is_empty() {
            bail!("Redis URL {url:?} has no host");
        }
        let password = auth.and_then(|auth| {
            auth.split_once(':')
                .map(|(_, password)| password.to_string())
        });
        Ok(Self {
            host,
            port,
            password,
        })
    }

    pub async fn ping(&self) -> Result<()> {
        let mut stream = self.connect().await?;
        self.authenticate(&mut stream).await?;
        let reply = command(&mut stream, &["PING"]).await?;
        if reply.as_status() != Some("PONG") {
            bail!("Redis PING returned {reply:?}");
        }
        Ok(())
    }

    /// `SELECT index; FLUSHDB` on a fresh connection.
    pub async fn flush_db(&self, index: i64) -> Result<()> {
        if index < 0 {
            bail!("Redis database index must be >= 0, got {index}");
        }
        let mut stream = self.connect().await?;
        self.authenticate(&mut stream).await?;
        let select = command(&mut stream, &["SELECT", &index.to_string()]).await?;
        if select.as_status() != Some("OK") {
            bail!("Redis SELECT {index} returned {select:?}");
        }
        let flush = command(&mut stream, &["FLUSHDB"]).await?;
        if flush.as_status() != Some("OK") {
            bail!("Redis FLUSHDB on db {index} returned {flush:?}");
        }
        Ok(())
    }

    async fn connect(&self) -> Result<TcpStream> {
        TcpStream::connect((self.host.as_str(), self.port))
            .await
            .with_context(|| format!("connecting to Redis at {}:{}", self.host, self.port))
    }

    async fn authenticate(&self, stream: &mut TcpStream) -> Result<()> {
        if let Some(password) = &self.password {
            let reply = command(stream, &["AUTH", password]).await?;
            if reply.as_status() != Some("OK") {
                bail!("Redis AUTH returned {reply:?}");
            }
        }
        Ok(())
    }
}

pub async fn command(stream: &mut TcpStream, args: &[&str]) -> Result<RespValue> {
    let mut request = format!("*{}\r\n", args.len());
    for arg in args {
        request.push_str(&format!("${}\r\n{}\r\n", arg.len(), arg));
    }
    stream
        .write_all(request.as_bytes())
        .await
        .context("writing a Redis command")?;
    let mut buffer = Vec::with_capacity(64);
    loop {
        let mut chunk = [0_u8; 512];
        let read = stream
            .read(&mut chunk)
            .await
            .context("reading a Redis reply")?;
        if read == 0 {
            bail!("Redis closed the connection before a full reply");
        }
        buffer.extend_from_slice(&chunk[..read]);
        let mut cursor = 0;
        if let Ok(Some(value)) = parse_resp(&buffer, &mut cursor) {
            return Ok(value);
        }
    }
}

/// Parse one RESP value from `input` starting at `cursor`.
///
/// Returns `Ok(None)` when the buffer ends before the value is complete.
pub fn parse_resp(input: &[u8], cursor: &mut usize) -> Result<Option<RespValue>> {
    let Some(&marker) = input.get(*cursor) else {
        return Ok(None);
    };
    let line_end = match find_crlf(input, *cursor) {
        Some(end) => end,
        None => return Ok(None),
    };
    let line =
        std::str::from_utf8(&input[*cursor + 1..line_end]).context("RESP line is not UTF-8")?;
    match marker {
        b'+' => {
            *cursor = line_end + 2;
            Ok(Some(RespValue::Simple(line.to_string())))
        }
        b'-' => {
            *cursor = line_end + 2;
            bail!("Redis error: {line}");
        }
        b':' => {
            *cursor = line_end + 2;
            Ok(Some(RespValue::Integer(line.parse().with_context(
                || format!("parsing RESP integer {line:?}"),
            )?)))
        }
        b'$' => {
            let length: i64 = line
                .parse()
                .with_context(|| format!("parsing RESP bulk length {line:?}"))?;
            if length < 0 {
                *cursor = line_end + 2;
                return Ok(Some(RespValue::Bulk(None)));
            }
            let start = line_end + 2;
            let end = start + length as usize;
            if input.len() < end + 2 {
                return Ok(None);
            }
            let text =
                std::str::from_utf8(&input[start..end]).context("RESP bulk value is not UTF-8")?;
            *cursor = end + 2;
            Ok(Some(RespValue::Bulk(Some(text.to_string()))))
        }
        b'*' => {
            let count: i64 = line
                .parse()
                .with_context(|| format!("parsing RESP array length {line:?}"))?;
            *cursor = line_end + 2;
            if count < 0 {
                return Ok(Some(RespValue::Array(Vec::new())));
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                match parse_resp(input, cursor)? {
                    Some(value) => items.push(value),
                    None => return Ok(None),
                }
            }
            Ok(Some(RespValue::Array(items)))
        }
        other => bail!("unknown RESP marker {:?}", other as char),
    }
}

fn find_crlf(input: &[u8], from: usize) -> Option<usize> {
    let mut index = from;
    while index + 1 < input.len() {
        if input[index] == b'\r' && input[index + 1] == b'\n' {
            return Some(index);
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_integer_and_bulk() {
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"+OK\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Simple("OK".to_string()))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b":42\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Integer(42))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"$5\r\nhello\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Bulk(Some("hello".to_string())))
        );
        let mut cursor = 0;
        assert_eq!(
            parse_resp(b"$-1\r\n", &mut cursor).expect("parse"),
            Some(RespValue::Bulk(None))
        );
    }

    #[test]
    fn parses_nested_arrays() {
        let mut cursor = 0;
        let value = parse_resp(b"*2\r\n$1\r\na\r\n:7\r\n", &mut cursor).expect("parse");
        assert_eq!(
            value,
            Some(RespValue::Array(vec![
                RespValue::Bulk(Some("a".to_string())),
                RespValue::Integer(7)
            ]))
        );
    }

    #[test]
    fn incomplete_input_returns_none() {
        let mut cursor = 0;
        assert_eq!(parse_resp(b"+OK", &mut cursor).expect("parse"), None);
        let mut cursor = 0;
        assert_eq!(parse_resp(b"$5\r\nhel", &mut cursor).expect("parse"), None);
    }

    #[test]
    fn errors_are_reported() {
        let mut cursor = 0;
        let error = parse_resp(b"-ERR bad\r\n", &mut cursor).expect_err("must fail");
        assert!(error.to_string().contains("ERR bad"));
    }

    #[test]
    fn redis_url_parsing() {
        let admin = RedisAdmin::from_url("redis://127.0.0.1:56379").expect("url");
        assert_eq!(admin.host, "127.0.0.1");
        assert_eq!(admin.port, 56379);
        assert!(admin.password.is_none());

        let admin = RedisAdmin::from_url("redis://user:secret@cache:6380/0").expect("url");
        assert_eq!(admin.host, "cache");
        assert_eq!(admin.port, 6380);
        assert_eq!(admin.password.as_deref(), Some("secret"));
    }
}
