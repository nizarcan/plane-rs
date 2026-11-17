// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The SQL of `MarkAllReadNotificationViewSet.create` (route 121,
//! `plane/app/views/notification/base.py:237-293`).
//!
//! ## The scan
//!
//! ```text
//! Notification.objects
//!   .filter(workspace__slug=slug, receiver_id=user, read_at__isnull=True)
//!   .select_related("workspace", "project", "triggered_by", "receiver")
//!   .order_by("snoozed_till", "-created_at")
//! ```
//!
//! then one `snoozed` filter (`base.py:251-254`), one `archived` filter
//! (`:257-260`) and, for a `type` that names one of three values, an
//! `entity_identifier IN (SELECT …)` subquery (`:263-286`). The three
//! subqueries are Django's own `values_list(..., flat=True)` querysets, nested
//! into the outer statement exactly as the list view nests them
//! (`src/notifications/base.rs`). `select_related` adds no predicate or
//! ordering, so it contributes no join that can change the row set.
//!
//! ## The update
//!
//! `Notification.objects.bulk_update(rows, ["read_at"], batch_size=100)`
//! (`:292`) writes `read_at` **only** — no `updated_at`, no `updated_by`, no
//! hooks — in `transaction.atomic(savepoint=False)` opened after the scan
//! (`django/db/models/query.py:922`). Each row's value is its own
//! `timezone.now()` (`base.py:289-290`), so the statement pairs ids with
//! timestamps. `unnest($1::uuid[], $2::timestamptz[])` is the `VALUES` list
//! Django's own `CASE WHEN id = … THEN …` compiles to, in the same order and
//! with one parameter pair per row.

use chrono::{DateTime, Utc};
use plane_db::Query;
use plane_db::plane_query;
use plane_query::sql::{Frag, Sql, Value};
use uuid::Uuid;

plane_query! {
    /// `bulk_update(..., ["read_at"], batch_size=100)` for one batch
    /// (`django/db/models/query.py:907-924`).
    ///
    /// Bind: `$1` the batch's ids, `$2` their `read_at` values, in scan order.
    /// Only `read_at` is assigned: no `updated_at`, no `updated_by`.
    NOTIFICATION_MARK_ALL_READ = "notifications_mark_all_read",
    "UPDATE \"notifications\" AS n SET \"read_at\" = t.\"read_at\" \
     FROM unnest($1::uuid[], $2::timestamptz[]) AS t(\"id\", \"read_at\") \
     WHERE n.\"id\" = t.\"id\"",
    []
}

/// The batch size Django's call passes (`base.py:292`).
pub const BATCH_SIZE: usize = 100;

/// `Q(snoozed_till__lt=now) | Q(snoozed_till__isnull=False)` when `snoozed`
/// is truthy, `Q(snoozed_till__gte=now) | Q(snoozed_till__isnull=True)`
/// otherwise (`base.py:251-254`).
///
/// A future `snoozed_till` matches **both** branches.
pub fn snoozed_filter(snoozed: bool, now: DateTime<Utc>) -> Frag {
    if snoozed {
        Frag::compare("n.\"snoozed_till\"", "<", Value::Timestamp(now))
            .or(Frag::is_not_null("n.\"snoozed_till\""))
    } else {
        Frag::compare("n.\"snoozed_till\"", ">=", Value::Timestamp(now))
            .or(Frag::is_null("n.\"snoozed_till\""))
    }
}

/// `archived_at IS NOT NULL` / `IS NULL` (`base.py:257-260`).
pub fn archived_filter(archived: bool) -> Frag {
    if archived {
        Frag::is_not_null("n.\"archived_at\"")
    } else {
        Frag::is_null("n.\"archived_at\"")
    }
}

/// One `type` branch, as `entity_identifier IN (SELECT …)` (`base.py:263-286`).
///
/// `watching` is `IssueSubscriber.objects.filter(workspace__slug=slug,
/// subscriber_id=user)`; `assigned` is the same over `IssueAssignee`; `created`
/// is `Issue.objects.filter(workspace__slug=slug, created_by=user)` and is only
/// reached for a non-guest (the guest `.none()` is handled before the
/// statement is built).
pub fn type_filter(kind: &str, slug: &str, user: Uuid) -> Option<Frag> {
    let mut frag = Frag::raw("n.\"entity_identifier\" IN (");
    match kind {
        "watching" => {
            frag.push_raw(
                "SELECT U0.\"issue_id\" FROM \"issue_subscribers\" U0 \
                 INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"subscriber_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
        }
        "assigned" => {
            frag.push_raw(
                "SELECT U0.\"issue_id\" FROM \"issue_assignees\" U0 \
                 INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"assignee_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
        }
        "created" => {
            frag.push_raw(
                "SELECT U0.\"id\" FROM \"issues\" U0 \
                 INNER JOIN \"workspaces\" U2 ON (U0.\"workspace_id\" = U2.\"id\") \
                 WHERE U0.\"deleted_at\" IS NULL AND U0.\"created_by_id\" = ",
            );
            frag.push(Frag::bind(Value::Uuid(user)));
            frag.push_raw(" AND U2.\"slug\" = ");
            frag.push(Frag::bind(Value::Text(slug.to_owned())));
        }
        _other => return None,
    }
    frag.push_raw(")");
    Some(frag)
}

/// The ordered id scan: `SELECT n.id … ORDER BY snoozed_till ASC, created_at
/// DESC`. Django's `ASC` is `NULLS LAST`, which Postgres' default already is.
///
/// Bind order: `$1` workspace slug, `$2` receiver id, `$3` `now`, then the
/// branch's two binds when [`type_filter`] matched.
pub fn scan(
    slug: &str,
    user: Uuid,
    now: DateTime<Utc>,
    snoozed: bool,
    archived: bool,
    type_: Option<Frag>,
) -> Sql {
    let mut predicate = Frag::raw("n.\"deleted_at\" IS NULL")
        .and(Frag::compare("n.\"receiver_id\"", "=", Value::Uuid(user)))
        .and(Frag::raw("w.\"slug\" = ").append(Frag::bind(Value::Text(slug.to_owned()))))
        .and(Frag::is_null("n.\"read_at\""))
        .and(snoozed_filter(snoozed, now))
        .and(archived_filter(archived));
    if let Some(type_) = type_ {
        predicate = predicate.and(type_);
    }
    let mut select = plane_query::sql::Select::new("notifications n");
    select.joins = vec![plane_query::sql::Join::inner(
        "workspaces",
        "w",
        Frag::raw("(n.\"workspace_id\" = w.\"id\")"),
    )];
    select.projection = vec![Frag::raw("n.\"id\"")];
    select.predicate = Some(predicate);
    select.order_by = vec![Frag::raw(
        "n.\"snoozed_till\" ASC NULLS LAST, n.\"created_at\" DESC",
    )];
    select.into_sql()
}

plane_query! {
    /// `UserNotificationPreference`'s five booleans are
    /// `UserNotificationPreferenceSerializer`'s only writable fields
    /// (`plane/app/serializers/notification.py:25-29`); the PATCH view saves the
    /// whole instance (`base.py:307-313`), so the statement rewrites every
    /// column after H01's `updated_by`/`updated_at`.
    ///
    /// Bind order: `$1..$13` the model columns in
    /// [`plane_db::models::notification::UserNotificationPreference::COLUMNS`]
    /// order (id excluded), `$14` the primary key.
    USER_NOTIFICATION_PREFERENCE_UPDATE = "notifications_user_notification_preference_update",
    "UPDATE \"user_notification_preferences\" SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"user_id\" = $6, \
     \"workspace_id\" = $7, \"project_id\" = $8, \"property_change\" = $9, \
     \"state_change\" = $10, \"comment\" = $11, \"mention\" = $12, \
     \"issue_completed\" = $13 \
     WHERE \"id\" = $14",
    []
}

/// Every catalogued statement of this group, in declaration order.
///
/// The scan is composed per request (four `snoozed`/`archived` combinations and
/// the three `type` branches), so only the fixed batch update is catalogued.
/// The guest probe is `super::queries::WORKSPACE_GUEST_EXISTS`, already listed
/// by that catalog.
pub static ALL_QUERIES: &[&Query] = &[
    &NOTIFICATION_MARK_ALL_READ,
    &USER_NOTIFICATION_PREFERENCE_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).single().expect("timestamp")
    }

    #[test]
    fn the_scan_orders_and_filters_like_django() {
        let user = Uuid::parse_str("78100240-9cfd-4782-8627-25ab039c6e9a").expect("uuid");
        let sql = scan("fix-workspace", user, at(0), false, false, None);
        assert!(
            sql.text
                .ends_with("ORDER BY n.\"snoozed_till\" ASC NULLS LAST, n.\"created_at\" DESC")
        );
        assert!(sql.text.contains("n.\"deleted_at\" IS NULL"));
        assert!(sql.text.contains("n.\"read_at\" IS NULL"));
        assert!(sql.text.contains("\"snoozed_till\" >= $3"));
        assert!(sql.text.contains("n.\"archived_at\" IS NULL"));
        assert_eq!(sql.params.len(), 3);
    }

    #[test]
    fn a_future_snooze_matches_both_branches() {
        // `true` is `< now OR IS NOT NULL`; `false` is `>= now OR IS NULL`.
        let on = snoozed_filter(true, at(0)).into_sql();
        assert_eq!(
            on.text,
            "(n.\"snoozed_till\" < $1) OR (n.\"snoozed_till\" IS NOT NULL)"
        );
        let off = snoozed_filter(false, at(0)).into_sql();
        assert_eq!(
            off.text,
            "(n.\"snoozed_till\" >= $1) OR (n.\"snoozed_till\" IS NULL)"
        );
    }

    #[test]
    fn the_type_branches_are_the_three_values() {
        let user = Uuid::parse_str("78100240-9cfd-4782-8627-25ab039c6e9a").expect("uuid");
        for kind in ["watching", "assigned", "created"] {
            let frag = type_filter(kind, "fix-workspace", user).expect(kind);
            assert!(
                frag.into_sql()
                    .text
                    .starts_with("n.\"entity_identifier\" IN (SELECT")
            );
        }
        assert!(type_filter("all", "fix-workspace", user).is_none());
        assert!(type_filter("subscribed", "fix-workspace", user).is_none());
    }
}
