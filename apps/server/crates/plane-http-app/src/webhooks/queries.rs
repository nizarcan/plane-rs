// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `webhooks` group.
//!
//! Three statements, one per shape `plane/app/views/webhook/base.py` runs:
//!
//! * [`WEBHOOKS_BY_WORKSPACE`] — `Webhook.objects.filter(workspace__slug=slug)`
//!   (`base.py:44-45`), ordered by the model's `Meta.ordering`
//!   (`-created_at`).
//! * [`WEBHOOK_BY_PK`] — `Webhook.objects.get(workspace__slug=slug, pk=pk)`
//!   (`base.py:63`), same row plus the primary-key lookup.
//! * [`WEBHOOK_LOGS`] — `WebhookLog.objects.filter(workspace__slug=slug,
//!   webhook=webhook_id)` (`base.py:127-128`).
//!
//! Every statement keeps Django's `workspaces` join (the `workspace__slug`
//! filter) and its soft-delete predicate (`SoftDeletionManager`,
//! `plane/db/mixins.py:57`). The `webhooks` column lists are the ones the
//! serializer actually reads: `WebhookSerializer` is a `DynamicBaseSerializer`
//! and both GET branches pass a fixed `fields=` tuple, so only
//! `id`, `url`, `created_at`, `updated_at`, `is_active`, `project`, `issue`,
//! `module`, `cycle` and `issue_comment` are selected — `secret_key` is never
//! read (it is also popped from the serialized dict when `show_secret_key` is
//! absent, `plane/app/serializers/webhook.py:44-52`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Webhook.objects.filter(workspace__slug=slug)`
    /// (`plane/app/views/webhook/base.py:44-45`).
    WEBHOOKS_BY_WORKSPACE = "webhooks_by_workspace",
    "SELECT w.id, w.url, w.created_at, w.updated_at, w.is_active, w.project, \
     w.issue, w.module, w.cycle, w.issue_comment \
     FROM webhooks w \
     INNER JOIN workspaces ws ON w.workspace_id = ws.id \
     WHERE w.deleted_at IS NULL AND ws.slug = $1 \
     ORDER BY w.created_at DESC",
    [
        ("id", Uuid),
        ("url", Varchar),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("is_active", Bool),
        ("project", Bool),
        ("issue", Bool),
        ("module", Bool),
        ("cycle", Bool),
        ("issue_comment", Bool),
    ]
}

plane_query! {
    /// `Webhook.objects.get(workspace__slug=slug, pk=pk)`
    /// (`plane/app/views/webhook/base.py:63`). A miss is Django's
    /// `DoesNotExist`, i.e. the family 404 body.
    WEBHOOK_BY_PK = "webhooks_by_pk",
    "SELECT w.id, w.url, w.created_at, w.updated_at, w.is_active, w.project, \
     w.issue, w.module, w.cycle, w.issue_comment \
     FROM webhooks w \
     INNER JOIN workspaces ws ON w.workspace_id = ws.id \
     WHERE w.deleted_at IS NULL AND ws.slug = $1 AND w.id = $2 \
     LIMIT 1",
    [
        ("id", Uuid),
        ("url", Varchar),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("is_active", Bool),
        ("project", Bool),
        ("issue", Bool),
        ("module", Bool),
        ("cycle", Bool),
        ("issue_comment", Bool),
    ]
}

plane_query! {
    /// `WebhookLog.objects.filter(workspace__slug=slug, webhook=webhook_id)`
    /// (`plane/app/views/webhook/base.py:127-128`), ordered by the model's
    /// `Meta.ordering` (`-created_at`).
    ///
    /// Columns are `WebhookLogSerializer`'s declaration order (the model's pk,
    /// no declared fields, then the model fields in model order): `webhook` is
    /// a plain `UUIDField`, not a relation, so it renders as a bare UUID.
    WEBHOOK_LOGS = "webhook_logs",
    "SELECT l.id, l.created_at, l.updated_at, l.deleted_at, l.webhook, \
     l.event_type, l.request_method, l.request_headers, l.request_body, \
     l.response_status, l.response_headers, l.response_body, l.retry_count, \
     l.created_by_id, l.updated_by_id, l.workspace_id \
     FROM webhook_logs l \
     INNER JOIN workspaces ws ON l.workspace_id = ws.id \
     WHERE l.deleted_at IS NULL AND ws.slug = $1 AND l.webhook = $2 \
     ORDER BY l.created_at DESC",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("webhook", Uuid),
        ("event_type", Varchar),
        ("request_method", Varchar),
        ("request_headers", Text),
        ("request_body", Text),
        ("response_status", Text),
        ("response_headers", Text),
        ("response_body", Text),
        ("retry_count", Int2),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("workspace_id", Uuid),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&WEBHOOKS_BY_WORKSPACE, &WEBHOOK_BY_PK, &WEBHOOK_LOGS];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
            assert!(!query.columns.is_empty());
        }
    }
}
