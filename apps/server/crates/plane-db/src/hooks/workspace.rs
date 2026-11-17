// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! H05 — `Workspace.delete` (`plane/db/models/workspace.py:156-176`,
//! MODEL_HOOKS.md:2132).
//!
//! ```python
//! def delete(self, using=None, soft=True, *args, **kwargs):
//!     result = super().delete(using=using, soft=soft, *args, **kwargs)
//!     if soft and hasattr(self, "deleted_at") and self.deleted_at:
//!         deletion_timestamp: int = int(self.deleted_at.timestamp())
//!         self.slug = f"{self.slug}__{deletion_timestamp}"
//!         self.save(update_fields=["slug"])
//!     return result
//! ```
//!
//! The base class's soft delete is H03 (one full-row `UPDATE`, then the
//! cascade task). This hook is the step **after** it: the slug gains a
//! `__<epoch>` suffix so the unique index frees the name for a new workspace
//! while the soft-deleted row keeps its identity.
//!
//! Three details a write path must reproduce:
//!
//! * `int(self.deleted_at.timestamp())` truncates toward zero, so a pre-1970
//!   `deleted_at` rounds up (`-1.5 s` becomes `-1`);
//! * the second save lists `slug` only, so `updated_at` and `updated_by_id`
//!   are **not** written even though H01 replaced `updated_by` in memory
//!   (`django/db/models/base.py:950-1008` filters `update_fields`);
//! * the suffix is appended to whatever the slug was, and `varchar(48)` is
//!   only checked by the database, so a long slug fails the UPDATE **after**
//!   the row is soft-deleted and the cascade published (`MODEL_HOOKS.md:506-
//!   516`) — a 500 with the deletion already committed.

use chrono::{DateTime, Utc};

/// The `save(update_fields=["slug"])` statement of the second write.
///
/// `$1` is the new slug, `$2` the workspace id.
pub const UPDATE_SLUG_SQL: &str = "UPDATE workspaces SET slug = $1 WHERE id = $2";

/// `f"{self.slug}__{int(self.deleted_at.timestamp())}"`.
///
/// `deleted_at` is H03's `timezone.now()`, the same instant written to the row;
/// the hook never reads the clock itself.
#[must_use]
pub fn after_soft_delete(slug: &str, deleted_at: DateTime<Utc>) -> String {
    // `timestamp()` is POSIX seconds as a float and `int()` truncates toward
    // zero; integer division of the microsecond count does the same.
    let epoch = deleted_at.timestamp_micros() / 1_000_000;
    format!("{slug}__{epoch}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64, micros: u32) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, micros).unwrap()
    }

    /// Recorded from the real `Workspace.delete` with
    /// `django.db.models.Model.save` patched out (`apps/api/.venv/bin/python`,
    /// CPython 3.12.13, Django 5.2.17).
    #[test]
    fn the_suffix_is_the_truncated_epoch() {
        assert_eq!(
            after_soft_delete("fix-workspace", at(1_700_000_123, 999_999)),
            "fix-workspace__1700000123"
        );
        assert_eq!(after_soft_delete("fix-deleted", at(0, 0)), "fix-deleted__0");
    }

    #[test]
    fn a_pre_epoch_instant_truncates_toward_zero() {
        // `int(-1.5)` is `-1` in Python, not `-2`.
        assert_eq!(after_soft_delete("w", at(-2, 500_000)), "w__-1");
        assert_eq!(after_soft_delete("w", at(-1, 999_999)), "w__0");
    }

    #[test]
    fn the_second_write_touches_only_the_slug() {
        assert_eq!(
            UPDATE_SLUG_SQL,
            "UPDATE workspaces SET slug = $1 WHERE id = $2"
        );
    }
}
