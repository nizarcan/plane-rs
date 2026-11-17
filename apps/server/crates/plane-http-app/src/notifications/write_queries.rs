// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `notifications` write actions (routes
//! 117–119).
//!
//! [`NOTIFICATION_BY_PK`] is `Notification.objects.get(workspace__slug=slug,
//! pk=pk, receiver=request.user)` (`plane/app/views/notification/base.py:158`,
//! `:170`, `:178`, `:186`, `:194`) with the `triggered_by` fields
//! `NotificationSerializer(instance).data` reads afterwards (`:164`, `:173`,
//! `:181`, `:189`, `:197`). Django runs the join and the nested serializer's
//! user query as two statements; one statement answers both, which is the shape
//! the list view already uses (`src/notifications/base.rs`).
//!
//! [`NOTIFICATION_FULL_SAVE`] is `instance.save()` with no `update_fields`
//! (`django/db/models/base.py:928-936`): every concrete column, after H01 set
//! `updated_by` (`plane/db/models/base.py:23-45`) and `auto_now` set
//! `updated_at`.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `Notification.objects.get(workspace__slug=slug, pk=pk,
    /// receiver=request.user)` (`base.py:158,170,178,186,194`).
    ///
    /// The manager's `deleted_at IS NULL` comes from
    /// `SoftDeletionManager.get_queryset` (`plane/db/mixins.py:56-58`);
    /// `workspace__slug` is an unfiltered join, exactly like the list view.
    /// `LIMIT 2` is enough to tell "one row" from `MultipleObjectsReturned`
    /// (impossible on a pk lookup).
    ///
    /// Bind order: `$1` notification id, `$2` workspace slug, `$3` receiver id.
    ///
    /// Columns 0–20 are [`plane_db::models::notification::Notification::from_row`]'s;
    /// 21–32 are the `UserLiteSerializer` block (`users` + `file_assets` +
    /// `workspaces`), decoded by `notifications::render::decode_triggered_by`.
    NOTIFICATION_BY_PK = "notifications_write_by_pk",
    "SELECT n.\"created_at\", n.\"updated_at\", n.\"created_by_id\", \
     n.\"updated_by_id\", n.\"deleted_at\", n.\"id\", n.\"workspace_id\", \
     n.\"project_id\", n.\"data\", n.\"entity_identifier\", n.\"entity_name\", \
     n.\"title\", n.\"message\", n.\"message_html\", n.\"message_stripped\", \
     n.\"sender\", n.\"triggered_by_id\", n.\"receiver_id\", n.\"read_at\", \
     n.\"snoozed_till\", n.\"archived_at\", \
     tb.\"id\" AS \"tb_id\", tb.\"first_name\" AS \"tb_first_name\", \
     tb.\"last_name\" AS \"tb_last_name\", tb.\"avatar\" AS \"tb_avatar\", \
     tb.\"avatar_asset_id\" AS \"tb_avatar_asset_id\", tb.\"is_bot\" AS \"tb_is_bot\", \
     tb.\"display_name\" AS \"tb_display_name\", \
     fa.\"id\" AS \"tb_avatar_asset_row_id\", \
     fa.\"entity_type\" AS \"tb_avatar_entity_type\", \
     fa.\"project_id\" AS \"tb_avatar_project_id\", \
     fa.\"issue_id\" AS \"tb_avatar_issue_id\", \
     w2.\"slug\" AS \"tb_avatar_workspace_slug\" \
     FROM \"notifications\" n \
     INNER JOIN \"workspaces\" w ON (n.\"workspace_id\" = w.\"id\") \
     LEFT OUTER JOIN \"users\" tb ON (n.\"triggered_by_id\" = tb.\"id\") \
     LEFT OUTER JOIN \"file_assets\" fa ON (tb.\"avatar_asset_id\" = fa.\"id\") \
     LEFT OUTER JOIN \"workspaces\" w2 ON (fa.\"workspace_id\" = w2.\"id\") \
     WHERE n.\"deleted_at\" IS NULL AND n.\"id\" = $1 \
       AND w.\"slug\" = $2 AND n.\"receiver_id\" = $3 \
     LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("data", Jsonb),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("title", Text),
        ("message", Jsonb),
        ("message_html", Text),
        ("message_stripped", Text),
        ("sender", Varchar),
        ("triggered_by_id", Uuid),
        ("receiver_id", Uuid),
        ("read_at", Timestamptz),
        ("snoozed_till", Timestamptz),
        ("archived_at", Timestamptz),
        ("tb_id", Uuid),
        ("tb_first_name", Varchar),
        ("tb_last_name", Varchar),
        ("tb_avatar", Text),
        ("tb_avatar_asset_id", Uuid),
        ("tb_is_bot", Bool),
        ("tb_display_name", Varchar),
        ("tb_avatar_asset_row_id", Uuid),
        ("tb_avatar_entity_type", Varchar),
        ("tb_avatar_project_id", Uuid),
        ("tb_avatar_issue_id", Uuid),
        ("tb_avatar_workspace_slug", Varchar),
    ]
}

plane_query! {
    /// `notification.save()` — a full-row `UPDATE` (`django/db/models/base.py:928-936`),
    /// after H01's `updated_by`/`updated_at`.
    ///
    /// Bind order: `$1..$20` the model columns in
    /// [`plane_db::models::notification::Notification::COLUMNS`] order (id
    /// excluded), `$21` the primary key.
    NOTIFICATION_FULL_SAVE = "notifications_write_full_save",
    "UPDATE \"notifications\" SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"workspace_id\" = $6, \
     \"project_id\" = $7, \"data\" = $8, \"entity_identifier\" = $9, \
     \"entity_name\" = $10, \"title\" = $11, \"message\" = $12, \
     \"message_html\" = $13, \"message_stripped\" = $14, \"sender\" = $15, \
     \"triggered_by_id\" = $16, \"receiver_id\" = $17, \"read_at\" = $18, \
     \"snoozed_till\" = $19, \"archived_at\" = $20 \
     WHERE \"id\" = $21",
    []
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[&NOTIFICATION_BY_PK, &NOTIFICATION_FULL_SAVE];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
        assert_eq!(NOTIFICATION_BY_PK.columns.len(), 33);
    }
}
