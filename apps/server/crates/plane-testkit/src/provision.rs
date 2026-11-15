// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Starting and stopping the two targets for one run.
//!
//! Each target gets its own free port in the configured range, its own
//! database clone, Redis database index and RabbitMQ vhost. The process is
//! started in its own process group so killing it also kills the gunicorn
//! workers it forks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::time::Instant;

use crate::config::TargetSpec;
use crate::http::{HttpClient, wait_healthy};
use crate::util::repo_root;

/// Fill the `{port}`, `{db}`, `{redis_db}`, `{vhost}` and `{repo}` placeholders.
pub fn render_template(template: &str, values: &BTreeMap<&str, String>) -> Result<String> {
    let mut rendered = template.to_string();
    for (key, value) in values {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    if rendered.contains('{') {
        bail!(
            "unrendered placeholder in {template:?} -> {rendered:?} (known: {})",
            values.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(rendered)
}

pub struct PortAllocator {
    first: u16,
    last: u16,
    used: BTreeSet<u16>,
}

impl PortAllocator {
    pub fn new(first: u16, last: u16) -> Self {
        Self {
            first,
            last,
            used: BTreeSet::new(),
        }
    }

    pub async fn allocate(&mut self) -> Result<u16> {
        for port in self.first..=self.last {
            if self.used.contains(&port) {
                continue;
            }
            match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(listener) => {
                    drop(listener);
                    self.used.insert(port);
                    return Ok(port);
                }
                Err(_) => continue,
            }
        }
        bail!(
            "no free port in {}..={} for a target",
            self.first,
            self.last
        )
    }
}

pub struct TargetInstance {
    pub name: String,
    pub port: u16,
    pub base_url: String,
    pub log_path: PathBuf,
    child: Child,
    stopped: bool,
}

impl TargetInstance {
    pub async fn start(
        name: &str,
        spec: &TargetSpec,
        ctx: &BTreeMap<&str, String>,
        log_dir: &Path,
    ) -> Result<Self> {
        let port: u16 = ctx
            .get("port")
            .ok_or_else(|| anyhow::anyhow!("target context has no port"))?
            .parse()
            .context("target context port")?;
        let argv: Vec<String> = spec
            .command
            .iter()
            .map(|arg| render_template(arg, ctx))
            .collect::<Result<_>>()?;
        let env: BTreeMap<String, String> = spec
            .env
            .iter()
            .map(|(key, value)| Ok((key.clone(), render_template(value, ctx)?)))
            .collect::<Result<_>>()?;
        let workdir = match &spec.workdir {
            Some(template) => PathBuf::from(render_template(template, ctx)?),
            None => repo_root(),
        };
        if !workdir.is_dir() {
            bail!(
                "target {name}: workdir {} does not exist",
                workdir.display()
            );
        }
        std::fs::create_dir_all(log_dir)
            .with_context(|| format!("creating log directory {}", log_dir.display()))?;
        let log_path = log_dir.join(format!("{name}.log"));
        let stdout = std::fs::File::create(&log_path)
            .with_context(|| format!("creating {}", log_path.display()))?;
        let stderr = stdout.try_clone().context("cloning the log handle")?;

        let program = &argv[0];
        let mut command = Command::new(program);
        command
            .args(&argv[1..])
            .envs(&env)
            .current_dir(&workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let child = command.spawn().with_context(|| {
            format!(
                "starting target {name} ({program} {:?}) in {}",
                &argv[1..],
                workdir.display()
            )
        })?;
        tracing::info!(
            "started target {name} pid {} on port {port}, log {}",
            child.id(),
            log_path.display()
        );
        Ok(Self {
            name: name.to_string(),
            port,
            base_url: format!("http://127.0.0.1:{port}"),
            log_path,
            child,
            stopped: false,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub async fn wait_healthy(&mut self, health_path: &str, timeout: Duration) -> Result<()> {
        let client = HttpClient::new(self.base_url.clone())?;
        wait_healthy(
            &client,
            health_path,
            timeout,
            &mut self.child,
            &self.log_path,
        )
        .await
        .with_context(|| format!("waiting for target {}", self.name))
    }

    pub async fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        if self.child.try_wait().context("polling target")?.is_some() {
            return Ok(());
        }
        let pgid = self.child.id();
        // Negative PID = the whole process group. `kill` is a system binary,
        // so no unsafe libc call is needed (the workspace forbids `unsafe`).
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{pgid}"))
            .status();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if self.child.try_wait().context("polling target")?.is_some() {
                tracing::info!("stopped target {} (pid {pgid})", self.name);
                return Ok(());
            }
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(format!("-{pgid}"))
            .status();
        let _ = self.child.wait();
        tracing::warn!("force-killed target {} (pid {pgid})", self.name);
        Ok(())
    }
}

impl Drop for TargetInstance {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(format!("-{}", self.child.id()))
            .status();
        let _ = self.child.try_wait();
    }
}

/// Wait for a TCP port to accept connections (Postgres, Redis, RabbitMQ).
pub async fn wait_tcp(name: &str, host: &str, port: u16, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match tokio::net::TcpStream::connect((host, port)).await {
            Ok(_) => return Ok(()),
            Err(error) => {
                if Instant::now() >= deadline {
                    return Err(error).with_context(|| {
                        format!("{name} at {host}:{port} never accepted connections")
                    });
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }
    }
}

/// Parse `host:port` out of a URL, for readiness probes.
pub fn host_port(url: &str, default_port: u16) -> (String, u16) {
    let rest = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url);
    let host_port = rest.rsplit('@').next().unwrap_or(rest);
    match host_port.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(default_port)),
        None => (host_port.to_string(), default_port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_rendered() {
        let values = BTreeMap::from([
            ("port", "18001".to_string()),
            ("db", "dt_ab_a".to_string()),
            ("redis_db", "1".to_string()),
            ("vhost", "dt_ab_a".to_string()),
            ("repo", "/repo".to_string()),
        ]);
        assert_eq!(
            render_template("PLANE_DB={db}", &values).expect("render"),
            "PLANE_DB=dt_ab_a"
        );
        assert_eq!(
            render_template("{repo}/apps/server/dev/django-serve.sh {port}", &values)
                .expect("render"),
            "/repo/apps/server/dev/django-serve.sh 18001"
        );
        assert!(render_template("{unknown}", &values).is_err());
    }

    #[test]
    fn host_and_port_are_extracted() {
        assert_eq!(
            host_port("postgresql://plane:plane@127.0.0.1:55432/plane", 5432),
            ("127.0.0.1".to_string(), 55432)
        );
        assert_eq!(
            host_port("http://localhost:55673", 80),
            ("localhost".to_string(), 55673)
        );
        assert_eq!(host_port("amqp://host", 5672), ("host".to_string(), 5672));
    }
}
