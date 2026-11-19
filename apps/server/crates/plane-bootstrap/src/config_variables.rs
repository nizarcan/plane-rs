// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's `instance_config_variables` table
//! (`plane/utils/instance_config_variables/core.py:8-263`), recorded by
//! `tools/boot/gen_instance_config.py`.
//!
//! The JSON is the list `manage.py configure_instance` iterates: key, category,
//! `is_encrypted`, and the default of `os.environ.get(key, default)` (Django's
//! `os.environ.get(key)` is `default: null`). The generator parses the Python
//! source instead of importing it, so the committed defaults cannot pick up the
//! generating shell's environment. `--check` fails when the list changes.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ConfigVariable {
    pub key: String,
    pub category: String,
    pub is_encrypted: bool,
    /// `None` is `os.environ.get(key)` with no default.
    pub default: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Document {
    variables: Vec<ConfigVariable>,
}

const VARIABLES_JSON: &str = include_str!("../assets/instance_config_variables.json");

/// Every row `configure_instance` seeds, in Django's order.
///
/// # Panics
///
/// The embedded document is a build-time input checked by
/// `tools/boot/gen_instance_config.py --check`; a malformed one is a broken
/// build, not a runtime condition.
#[must_use]
pub fn variables() -> &'static [ConfigVariable] {
    static VARIABLES: std::sync::OnceLock<Vec<ConfigVariable>> = std::sync::OnceLock::new();
    VARIABLES.get_or_init(|| {
        let document: Document = serde_json::from_str(VARIABLES_JSON)
            .expect("assets/instance_config_variables.json is generated and valid");
        document.variables
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thirty_six_variables_with_unique_keys() {
        let variables = variables();
        assert_eq!(variables.len(), 36);
        let mut keys: Vec<&str> = variables.iter().map(|entry| entry.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), 36);
    }

    #[test]
    fn encrypted_and_defaulted_entries_match_django() {
        let variables = variables();
        let google = variables
            .iter()
            .find(|entry| entry.key == "GOOGLE_CLIENT_SECRET")
            .expect("GOOGLE_CLIENT_SECRET");
        assert!(google.is_encrypted);
        assert_eq!(google.default, None);
        let signup = variables
            .iter()
            .find(|entry| entry.key == "ENABLE_SIGNUP")
            .expect("ENABLE_SIGNUP");
        assert!(!signup.is_encrypted);
        assert_eq!(signup.default.as_deref(), Some("1"));
        let port = variables
            .iter()
            .find(|entry| entry.key == "EMAIL_PORT")
            .expect("EMAIL_PORT");
        assert_eq!(port.default.as_deref(), Some("587"));
    }
}
