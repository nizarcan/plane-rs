// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `webhooks` writes.
//!
//! | Statement | Python |
//! | --- | --- |
//! | [`WEBHOOK_BY_PK_FULL`] | `Webhook.objects.get(workspace__slug=slug, pk=pk)` (`plane/app/views/webhook/base.py:63`, `:83`, `:116`) |
//! | [`WEBHOOK_INSERT`] | `Webhook.objects.create(**validated_data)` (`serializers/webhook.py:72`) |
//! | [`WEBHOOK_UPDATE`] | `instance.save()` with no `update_fields` (`django/db/models/base.py:928-936`) |
//!
//! `Webhook` has no `save()` override (`MODEL_HOOKS.md:1296-1315`), so a write
//! is H01 (`created_by`/`updated_by`) plus the ORM's `auto_now_add`/`auto_now`
//! timestamps, and then every column is written verbatim. Column order is
//! [`plane_db::models::Webhook::COLUMNS`], which is also `from_row`'s order:
//! `created_at`, `updated_at`, `created_by_id`, `updated_by_id`, `deleted_at`,
//! `id`, `workspace_id`, `url`, `is_active`, `secret_key`, `project`, `issue`,
//! `module`, `cycle`, `issue_comment`, `is_internal`, `version`.
//!
//! There is no uniqueness query: the live-row conflict is the partial index
//! `webhook_url_unique_url_when_deleted_at_null`
//! (`plane/db/models/webhook.py:56-62`), which surfaces as an `IntegrityError`
//! from the INSERT and is matched on psycopg's message text
//! (`views/webhook/base.py:33-39`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Webhook.objects.get(workspace__slug=slug, pk=pk)` — the create/regenerate
    /// (set A) and PATCH/DELETE (set B) lookup.
    ///
    /// The manager's `deleted_at IS NULL` comes from
    /// `SoftDeletionManager.get_queryset` (`plane/db/mixins.py:56-58`), and the
    /// `workspaces` join is unfiltered, exactly like the read views: a member of
    /// a soft-deleted workspace passes the permission check and then 404s here.
    ///
    /// `LIMIT 2` distinguishes one row from `MultipleObjectsReturned` the way
    /// Django's `get()` does; a pk lookup can return at most one.
    ///
    /// Bind order: `$1` workspace slug, `$2` webhook id.
    WEBHOOK_BY_PK_FULL = "webhooks_write_by_pk_full",
    "SELECT w.\"created_at\", w.\"updated_at\", w.\"created_by_id\", \
     w.\"updated_by_id\", w.\"deleted_at\", w.\"id\", w.\"workspace_id\", \
     w.\"url\", w.\"is_active\", w.\"secret_key\", w.\"project\", w.\"issue\", \
     w.\"module\", w.\"cycle\", w.\"issue_comment\", w.\"is_internal\", \
     w.\"version\" \
     FROM \"webhooks\" w \
     INNER JOIN \"workspaces\" ws ON (w.\"workspace_id\" = ws.\"id\") \
     WHERE w.\"deleted_at\" IS NULL AND ws.\"slug\" = $1 AND w.\"id\" = $2 \
     LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("url", Varchar),
        ("is_active", Bool),
        ("secret_key", Varchar),
        ("project", Bool),
        ("issue", Bool),
        ("module", Bool),
        ("cycle", Bool),
        ("issue_comment", Bool),
        ("is_internal", Bool),
        ("version", Varchar),
    ]
}

plane_query! {
    /// `Webhook.objects.create(**validated_data)` (`serializers/webhook.py:72`)
    /// — one `INSERT` with every column, because `Model.save(force_insert=True)`
    /// writes the whole row.
    ///
    /// `created_at`/`updated_at` are the ORM's two separate `timezone.now()`
    /// calls (`auto_now_add` then `auto_now`), so they are bound, not defaulted;
    /// `created_by_id` is the request user and `updated_by_id` is `NULL` (H01
    /// `Audit::on_create`); `deleted_at` is `NULL`; `secret_key` is either the
    /// generated value or the caller's (the field is read-only, so the
    /// serializer never supplies one — the default is `generate_token`).
    ///
    /// Bind order: `$1..$17` in [`plane_db::models::Webhook::COLUMNS`] order.
    WEBHOOK_INSERT = "webhooks_write_insert",
    "INSERT INTO \"webhooks\" \
     (\"created_at\", \"updated_at\", \"created_by_id\", \"updated_by_id\", \
      \"deleted_at\", \"id\", \"workspace_id\", \"url\", \"is_active\", \
      \"secret_key\", \"project\", \"issue\", \"module\", \"cycle\", \
      \"issue_comment\", \"is_internal\", \"version\") \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
       $15, $16, $17)",
    []
}

plane_query! {
    /// `webhook.save()` with no `update_fields`
    /// (`django/db/models/base.py:928-936`): every concrete column is rewritten,
    /// including the ones the caller did not touch (H01 set `updated_by`, and
    /// `auto_now` set `updated_at`).
    ///
    /// Set B's PATCH writes a body `created_by` verbatim (the FK is writable);
    /// `WebhookSecretRegenerateEndpoint` (set A) changes only `secret_key` but
    /// still full-saves.
    ///
    /// Bind order: `$1..$16` the columns of
    /// [`plane_db::models::Webhook::COLUMNS`] except `id`, then `$17` the
    /// primary key.
    WEBHOOK_UPDATE = "webhooks_write_update",
    "UPDATE \"webhooks\" SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"workspace_id\" = $6, \
     \"url\" = $7, \"is_active\" = $8, \"secret_key\" = $9, \"project\" = $10, \
     \"issue\" = $11, \"module\" = $12, \"cycle\" = $13, \
     \"issue_comment\" = $14, \"is_internal\" = $15, \"version\" = $16 \
     WHERE \"id\" = $17",
    []
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&WEBHOOK_BY_PK_FULL, &WEBHOOK_INSERT, &WEBHOOK_UPDATE];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            assert!(!query.sql.is_empty());
        }
        assert_eq!(
            WEBHOOK_BY_PK_FULL.columns.len(),
            plane_db::models::Webhook::COLUMN_COUNT
        );
    }
}
