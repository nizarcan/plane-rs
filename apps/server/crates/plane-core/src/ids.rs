// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared identifier newtypes.
//!
//! The database schema uses UUID primary keys and 128-character session keys
//! (`plane/db/models/session.py:35-43`); wrapping them keeps write paths from
//! mixing a user id with an instance id at compile time.

use std::fmt;

use uuid::Uuid;

use serde::{Deserialize, Serialize};

/// `users.id` / `created_by_id` / … — a UUID primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<Uuid> for UserId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

/// `instances.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InstanceId(pub Uuid);

impl InstanceId {
    pub fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<Uuid> for InstanceId {
    fn from(value: Uuid) -> Self {
        Self(value)
    }
}

/// `sessions.session_key`: 128 characters from `a-z0-9`
/// (`plane/db/models/session.py:13,35-43`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionKey(String);

impl SessionKey {
    /// Validate the alphabet and length the Django store generates. Existing
    /// rows created by Django always satisfy this.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if value.len() == 128
            && value
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Accept any key without validation, for values read back from the
    /// database (a deployment may have changed the key length).
    pub fn from_database(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SessionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_validation() {
        let valid = "a".repeat(128);
        assert!(SessionKey::new(valid.clone()).is_some());
        assert!(SessionKey::new("A".repeat(128)).is_none());
        assert!(SessionKey::new("a".repeat(127)).is_none());
        assert!(SessionKey::new("a".repeat(129)).is_none());
        assert!(SessionKey::new("-".repeat(128)).is_none());
        assert_eq!(SessionKey::new(valid.clone()).unwrap().as_str(), valid);
    }
}
