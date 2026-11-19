// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `register_instance` and `configure_instance`
//! (`plane/license/management/commands/`, `plans/CUTOVER.md` §3).
//!
//! `register_instance` creates the single `Instance` row on a fresh install and
//! extends it on every later boot. The fields it writes are
//! `register_instance.py:60-87`, the version lookup is `:28-51` (GitHub's
//! `releases/latest`, a 10 s timeout through `plane-net`'s `GITHUB_RELEASES`
//! preset, every error swallowed into the fallback), and the machine signature
//! is only checked for emptiness (`:64-65`); Django never stores it.
//!
//! `register_instance.py:90` also publishes `push_instance_metrics.delay()` on
//! **every** run, create or extend. The task is Rust-owned (D34 amendment), so
//! the step inserts a `plane_jobs.jobs` row through
//! `plane_jobs::route::dispatch` on the boot connection; a process without a
//! publisher is fine because the task is never Celery-owned.
//!
//! `configure_instance` (`configure_instance.py:19-40`) seeds the 36
//! `InstanceConfiguration` rows, encrypting the ones Django marks
//! `is_encrypted` with the same Fernet token (`plane/license/utils/encryption.py`
//! ported in `plane_auth::fernet`). `SECRET_KEY` is mandatory, exactly as the
//! Python command's `CommandError`.

use plane_auth::fernet::encrypt_data;
use plane_auth::random::{get_random_string, hex};
use plane_core::config::Settings;
use plane_net::{HttpEngine, Method, OutboundRequest, SsrfPolicy, preset};
use tokio_postgres::Client;
use uuid::Uuid;

use crate::Result;
use crate::config_variables;

/// `https://api.github.com/repos/makeplane/plane/releases/latest`
/// (`register_instance.py:42-45`).
pub const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/makeplane/plane/releases/latest";

/// The version embedded from `apps/api/package.json` (`"version": "1.5.0"`),
/// because the distroless image ships no files (`plans/CUTOVER.md` §3). The
/// unit test below pins it to the real file at build time.
pub const PACKAGE_VERSION: &str = "1.5.0";

/// The GitHub JSON `tag_name`, or the fallback. `None` is Python's `None` (a
/// JSON `null` tag), which Django stores as SQL `NULL`.
async fn latest_version(latest_url: &str, fallback: &str) -> Option<String> {
    // `requests.get(url, timeout=10)`: the preset carries the 10 s per-socket
    // timeout, the 30-redirect limit and `python-requests/2.33.0`; the URL is a
    // constant, not user input, so it is not SSRF-pinned (`preset.rs:306-316`).
    let response = HttpEngine::system()
        .fetch(
            &preset::GITHUB_RELEASES,
            Method::GET,
            &SsrfPolicy::strict(),
            latest_url,
            &OutboundRequest::new(),
        )
        .await;
    let (response, _) = match response {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(target: "plane.boot", %error, "GitHub releases/latest lookup failed");
            println!("Error checking for latest version");
            return Some(fallback.to_string());
        }
    };
    if !(200..300).contains(&response.status) {
        tracing::warn!(
            target: "plane.boot",
            status = response.status,
            "GitHub releases/latest answered a non-2xx status"
        );
        println!("Error checking for latest version");
        return Some(fallback.to_string());
    }
    let Ok(data) = serde_json::from_slice::<serde_json::Value>(response.bytes()) else {
        tracing::warn!(target: "plane.boot", "GitHub releases/latest was not JSON");
        println!("Error checking for latest version");
        return Some(fallback.to_string());
    };
    let Some(tag) = data.get("tag_name") else {
        // `data.get("tag_name", fallback_version)`: the key is absent.
        return Some(fallback.to_string());
    };
    match tag {
        serde_json::Value::Null => None,
        serde_json::Value::String(value) => Some(value.clone()),
        // Django assigns whatever JSON produced to a CharField, whose
        // `get_prep_value` stringifies a non-string (`True` -> `"True"`).
        other => Some(match other {
            serde_json::Value::Bool(value) => {
                if *value {
                    "True".to_string()
                } else {
                    "False".to_string()
                }
            }
            value => value.to_string(),
        }),
    }
}

/// `check_for_current_version` (`register_instance.py:28-38`): `APP_VERSION`
/// when set and non-empty, else `package.json`'s `version`.
#[must_use]
pub fn current_version(settings: &Settings) -> String {
    match settings.runtime.app_version.as_deref() {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => PACKAGE_VERSION.to_string(),
    }
}

/// The entrypoint's `MACHINE_SIGNATURE`. The Python entrypoint hashes hostname,
/// MAC, CPU, memory and disk; Django only validates that the argument is
/// non-empty and never persists it (`register_instance.py:62-65`), and a
/// distroless image cannot read `/proc`. A 64-character digest of the hostname
/// therefore has the same observable effect.
#[must_use]
pub fn machine_signature() -> String {
    let hostname = gethostname();
    let mut bytes = [0u8; 32];
    let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, hostname.as_bytes());
    bytes.copy_from_slice(digest.as_ref());
    hex(&bytes)
}

fn gethostname() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string())
}

/// `Instance.objects.first()` under the default (soft-delete-filtered) manager.
async fn first_instance(client: &Client) -> Result<Option<Uuid>> {
    let row = client
        .query_opt(
            "SELECT id FROM instances WHERE deleted_at IS NULL \
             ORDER BY created_at DESC LIMIT 1",
            &[],
        )
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// `register_instance <machine_signature>` (`register_instance.py:53-92`).
///
/// Prints Django's two success lines. Returns `true` when the row was created.
/// A `machine_signature` that is empty is Django's `CommandError`; boot always
/// passes [`machine_signature`].
pub async fn register_instance(
    client: &Client,
    settings: &Settings,
    machine_signature: &str,
) -> Result<bool> {
    let latest_url = std::env::var("PLANE_RS_GITHUB_RELEASES_URL")
        .unwrap_or_else(|_| GITHUB_RELEASES_URL.to_string());
    let current = current_version(settings);
    let latest = latest_version(&latest_url, &current).await;
    let is_test = settings.integrations.is_test;
    let mut registered = false;
    match first_instance(client).await? {
        Some(id) => {
            println!("Instance already registered");
            // `instance.save()` writes every column; the five assignments
            // below (`register_instance.py:82-87`) are the only ones that can
            // differ, so the resulting row is identical.
            client
                .execute(
                    // `last_checked_at = timezone.now()` and the `auto_now`
                    // `updated_at` are two separate Python calls, so they
                    // differ by microseconds; `now()` and `clock_timestamp()`
                    // reproduce that (and keep the two observable fields
                    // distinguishable, as Django's are).
                    "UPDATE instances SET last_checked_at = now(), current_version = $2, \
                     latest_version = $3, is_test = $4, edition = 'PLANE_COMMUNITY', \
                     updated_at = clock_timestamp() WHERE id = $1",
                    &[&id, &current, &latest, &is_test],
                )
                .await?;
        }
        None => {
            if machine_signature.is_empty() {
                return Err(crate::BootError::Message(
                    "Machine signature is required".to_string(),
                ));
            }
            // `secrets.token_hex(12)`.
            let instance_id = get_random_string(24, "0123456789abcdef");
            let id = Uuid::new_v4();
            client
                .execute(
                    "INSERT INTO instances (id, created_at, updated_at, created_by_id, \
                      updated_by_id, deleted_at, instance_name, whitelist_emails, \
                      instance_id, current_version, latest_version, edition, domain, \
                      last_checked_at, namespace, is_telemetry_enabled, \
                      is_support_required, is_setup_done, is_signup_screen_visited, \
                      is_verified, is_test, is_current_version_deprecated) \
                     VALUES ($1, now(), now(), NULL, NULL, NULL, \
                      'Plane Community Edition', NULL, $2, $3, $4, 'PLANE_COMMUNITY', \
                      '', now(), NULL, true, true, false, false, false, $5, false)",
                    &[&id, &instance_id, &current, &latest, &is_test],
                )
                .await?;
            println!("Instance registered");
            registered = true;
        }
    }
    // `push_instance_metrics.delay()` (`register_instance.py:90`): the typed
    // Celery message is built for the coexistence wire format and
    // `Job::from_message` is the queue representation the Rust task receives
    // (D25). Nothing on the Celery side owns it, so no publisher is supplied.
    let task = plane_jobs::celery::task_name::PUSH_INSTANCE_METRICS;
    let message = plane_jobs::celery::tasks::push_instance_metrics();
    let job = plane_jobs::queue::Job::from_message(&message);
    plane_jobs::route::dispatch(task, &job, &message, None, Some(client))
        .await
        .map_err(|error| {
            crate::BootError::Message(format!("enqueueing push_instance_metrics: {error}"))
        })?;
    Ok(registered)
}

/// `configure_instance` (`configure_instance.py:19-40`).
///
/// The `SECRET_KEY` check happens before the loop: Django raises
/// `CommandError(f"{item} env variable is required.")` when it is unset.
/// `get_or_create(key=...)` uses the soft-delete-filtered manager, so a
/// soft-deleted row does not count as existing and the insert hits the unique
/// constraint, exactly as Django's would.
pub async fn configure_instance(client: &Client, settings: &Settings) -> Result<()> {
    if settings.secret_key.is_random() || settings.secret_key.value().is_empty() {
        // `os.environ.get("SECRET_KEY")` is falsy for unset and empty.
        return Err(crate::BootError::Message(
            "SECRET_KEY env variable is required.".to_string(),
        ));
    }
    let variables = config_variables::variables();
    for variable in variables {
        let exists = client
            .query_opt(
                "SELECT 1 FROM instance_configurations \
                 WHERE key = $1 AND deleted_at IS NULL",
                &[&variable.key],
            )
            .await?
            .is_some();
        if exists {
            println!("{} configuration already exists", variable.key);
            continue;
        }
        // `os.environ.get(key, default)`: an empty value is a value.
        let raw = std::env::var(&variable.key)
            .ok()
            .or_else(|| variable.default.clone());
        let value: Option<String> = if variable.is_encrypted {
            // `encrypt_data(None)` is `""` (`encryption.py:20-30`).
            Some(encrypt_data(
                settings.secret_key.value(),
                raw.as_deref().unwrap_or_default(),
            ))
        } else {
            raw
        };
        client
            .execute(
                "INSERT INTO instance_configurations (id, created_at, updated_at, \
                  created_by_id, updated_by_id, deleted_at, key, value, category, \
                  is_encrypted) \
                 VALUES ($1, now(), now(), NULL, NULL, NULL, $2, $3, $4, $5)",
                &[
                    &Uuid::new_v4(),
                    &variable.key,
                    &value,
                    &variable.category,
                    &variable.is_encrypted,
                ],
            )
            .await?;
        println!(
            "{} loaded with value from environment variable.",
            variable.key
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded version is the oracle's `apps/api/package.json` version.
    /// The test reads the cached oracle; without it (a plain `cargo test` in a
    /// checkout that never ran `oracle.sh fetch`) it prints a notice and skips.
    #[test]
    fn package_version_matches_apps_api() {
        let path = plane_oracle::api_dir().join("package.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!(
                "package version test: {} is absent; run tools/upstream/oracle.sh fetch",
                path.display()
            );
            return;
        };
        let package: serde_json::Value =
            serde_json::from_str(&text).expect("apps/api/package.json parses");
        assert_eq!(package["version"], PACKAGE_VERSION);
    }

    #[test]
    fn machine_signature_is_hex_and_not_empty() {
        let signature = machine_signature();
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    }
}
