// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H01 — `BaseModel.save`'s audit columns (`plane/db/models/base.py:23-45`).
//!
//! ```python
//! if created_by_id: self.created_by_id = created_by_id
//! elif user is None or user.is_anonymous: self.created_by = None; self.updated_by = None
//! elif self._state.adding: self.created_by = user; self.updated_by = None
//! else: self.updated_by = user
//! ```
//!
//! `crum`'s current user is the request user on a DRF request and `None` in a
//! task (`crum.get_current_user`), so a write path passes `Some(request.user)`
//! and a task passes `None`.

use uuid::Uuid;

/// The two audit columns to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audit {
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
}

impl Audit {
    /// `self._state.adding` is true.
    #[must_use]
    pub fn on_create(user: Option<Uuid>) -> Self {
        Self {
            created_by_id: user,
            updated_by_id: None,
        }
    }

    /// The row already existed: only `updated_by` changes; `created_by` keeps
    /// its stored value (a write path binds the stored one).
    #[must_use]
    pub fn on_update(user: Option<Uuid>) -> Option<Uuid> {
        user
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_sets_created_by_and_leaves_updated_by_null() {
        let user = Uuid::nil();
        assert_eq!(
            Audit::on_create(Some(user)),
            Audit {
                created_by_id: Some(user),
                updated_by_id: None
            }
        );
        assert_eq!(
            Audit::on_create(None),
            Audit {
                created_by_id: None,
                updated_by_id: None
            }
        );
        assert_eq!(Audit::on_update(Some(user)), Some(user));
        assert_eq!(Audit::on_update(None), None);
    }
}
