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

/// Source `script` in a clean bash and return every variable it exports.
///
/// `env -0` is used instead of parsing `export` lines: the script is free to
/// use quoting, and a value can contain anything but NUL. The script sees the
/// target's own `PLANE_DB`, `PLANE_REDIS_DB` and `PLANE_MQ_VHOST`, which is how
/// `dev/django-env.sh` selects a target's database and Redis index.
fn script_environment(
    script: &Path,
    ctx: &BTreeMap<&str, String>,
) -> Result<BTreeMap<String, String>> {
    if !script.is_file() {
        bail!("environment script {} does not exist", script.display());
    }
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg("set -a; source \"$1\" >/dev/null; env -0")
        .arg("difftest-env")
        .arg(script)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    for key in ["db", "redis_db", "vhost"] {
        if let Some(value) = ctx.get(key) {
            command.env(
                match key {
                    "db" => "PLANE_DB",
                    "redis_db" => "PLANE_REDIS_DB",
                    _ => "PLANE_MQ_VHOST",
                },
                value,
            );
        }
    }
    let output = command
        .output()
        .with_context(|| format!("running bash to source {}", script.display()))?;
    if !output.status.success() {
        bail!(
            "sourcing {} failed with {}: {}",
            script.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut env = BTreeMap::new();
    for entry in output.stdout.split(|byte| *byte == 0) {
        if entry.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(entry);
        if let Some((key, value)) = text.split_once('=') {
            env.insert(key.to_string(), value.to_string());
        }
    }
    Ok(env)
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
        // Variables exported by a sourced shell script first, then the
        // explicit `env:` template, which wins. `django` targets get their
        // environment from `dev/django-serve.sh`; a `rust` target uses
        // `env_from: dev/django-env.sh` so it sees exactly the same values.
        let mut env: BTreeMap<String, String> = match &spec.env_from {
            Some(template) => {
                let script = PathBuf::from(render_template(template, ctx)?);
                script_environment(&script, ctx).with_context(|| {
                    format!(
                        "target {name}: reading the environment from {}",
                        script.display()
                    )
                })?
            }
            None => BTreeMap::new(),
        };
        for (key, value) in &spec.env {
            env.insert(key.clone(), render_template(value, ctx)?);
        }
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
    fn script_environment_exports_variables() {
        let dir = std::env::temp_dir().join(format!("difftest-env-script-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let script = dir.join("env.sh");
        std::fs::write(
            &script,
            "echo ignored\n\
             export FOO='bar baz'\n\
             export PLANE_DB=$PLANE_DB\n\
             export PLANE_REDIS_DB=$PLANE_REDIS_DB\n",
        )
        .expect("write");
        let ctx = BTreeMap::from([
            ("db", "t_env".to_string()),
            ("redis_db", "3".to_string()),
            ("vhost", "v".to_string()),
        ]);
        let env = script_environment(&script, &ctx).expect("environment");
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar baz"));
        assert_eq!(env.get("PLANE_DB").map(String::as_str), Some("t_env"));
        assert_eq!(env.get("PLANE_REDIS_DB").map(String::as_str), Some("3"));
        // The child environment also carries the three PLANE_* knobs the
        // script was given, plus PATH and HOME.
        assert_eq!(env.get("PLANE_MQ_VHOST").map(String::as_str), Some("v"));
        assert!(env.contains_key("PATH"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_environment_script_is_an_error() {
        let missing = std::env::temp_dir().join("difftest-no-such-script.sh");
        let error = script_environment(&missing, &BTreeMap::new()).expect_err("must fail");
        assert!(error.to_string().contains("does not exist"));
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
