// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `PLANE_RS_BOOT_STEPS`: whether `plane-api` runs the entrypoint's mutating
//! setup steps.
//!
//! `apps/api/bin/docker-entrypoint-api.sh:24-33` runs `register_instance`,
//! `configure_instance`, `create_bucket` and `clear_cache` before gunicorn
//! binds. Production always does. The differential harness starts Django with
//! `dev/django-serve.sh`, which deliberately starts *after* those steps, so
//! skipping them on the Rust side is the only way both targets begin from the
//! same cloned fixture (`scenarios/targets.yaml` sets [`BootSteps::ENV`] to
//! `skip`; `tools/boot/README.md` and D34 explain why).
//!
//! The switch is opt-out so a deployment cannot lose the steps by omission: an
//! unset value runs every one of them, and anything other than `all`/`skip` is
//! a startup error naming the variable.

use crate::{BootError, Result};

/// `PLANE_RS_BOOT_STEPS`'s unset default and its two accepted values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootSteps {
    /// The default: run every mutating boot step, as the Docker entrypoint does.
    All,
    /// Test-harness only: skip the mutating steps and start from the database as
    /// the harness cloned it. `plane-api` logs a warning when this is set.
    Skip,
}

impl BootSteps {
    /// The environment variable that selects this switch (`ENV.md` §6).
    pub const ENV: &'static str = "PLANE_RS_BOOT_STEPS";

    /// Read [`BootSteps::ENV`] from the process environment.
    pub fn from_env() -> Result<Self> {
        match std::env::var(Self::ENV) {
            Ok(value) => Self::parse(Some(&value)),
            Err(std::env::VarError::NotPresent) => Ok(Self::All),
            Err(std::env::VarError::NotUnicode(_)) => Err(BootError::Message(format!(
                "{}=... is not valid UTF-8; expected `all` or `skip`",
                Self::ENV
            ))),
        }
    }

    /// Parse a value: `None` (unset) is [`BootSteps::All`]; `all` and `skip`
    /// are the only accepted strings. An empty value is not "unset": it is an
    /// error, like every other exact-value `PLANE_RS_` setting.
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("all") => Ok(Self::All),
            Some("skip") => Ok(Self::Skip),
            Some(other) => Err(BootError::Message(format!(
                "{ENV} must be `all` or `skip`, got {other:?}",
                ENV = Self::ENV
            ))),
        }
    }

    /// Whether the four mutating steps should run before the listener opens.
    #[must_use]
    pub fn runs_mutating(self) -> bool {
        matches!(self, Self::All)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_and_all_run_every_step() {
        assert_eq!(BootSteps::parse(None).expect("unset"), BootSteps::All);
        assert_eq!(BootSteps::parse(Some("all")).expect("all"), BootSteps::All);
        assert!(BootSteps::parse(None).expect("unset").runs_mutating());
    }

    #[test]
    fn skip_is_explicit() {
        let steps = BootSteps::parse(Some("skip")).expect("skip");
        assert_eq!(steps, BootSteps::Skip);
        assert!(!steps.runs_mutating());
    }

    #[test]
    fn anything_else_is_a_startup_error() {
        for value in ["", "SKIP", "none", "skip,all", "0"] {
            let error = BootSteps::parse(Some(value)).expect_err(value);
            let message = error.to_string();
            assert!(message.contains(BootSteps::ENV), "{message}");
            assert!(message.contains("`all` or `skip`"), "{message}");
        }
    }
}
