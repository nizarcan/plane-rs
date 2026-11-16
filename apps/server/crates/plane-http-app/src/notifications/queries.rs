// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `notifications` group.
//!
//! The notification list itself is dynamic (`snoozed`, `archived`, `read`,
//! `mentioned`, `type` and the paginator re-order), so it is composed with
//! `plane-query` in `src/notifications/base.rs`. These two statements are fixed.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `UnreadNotificationEndpoint.get` (`plane/app/views/notification/base.py:201-234`).
    ///
    /// The Python runs two `.count()` queries that differ only in the
    /// `sender__icontains="mentioned"` predicate; `COUNT(*) FILTER` answers
    /// both in one round trip with the same rows.
    ///
    /// Bind order: `$1` receiver id, `$2` workspace slug.
    UNREAD_COUNTS = "notifications_unread_counts",
    "SELECT \
       COUNT(*) FILTER (WHERE NOT (UPPER(n.\"sender\"::text) LIKE UPPER('%mentioned%'))) \
         AS \"total_unread_notifications_count\", \
       COUNT(*) FILTER (WHERE UPPER(n.\"sender\"::text) LIKE UPPER('%mentioned%')) \
         AS \"mention_unread_notifications_count\" \
     FROM \"notifications\" n \
     INNER JOIN \"workspaces\" w ON (n.\"workspace_id\" = w.\"id\") \
     WHERE n.\"deleted_at\" IS NULL AND n.\"archived_at\" IS NULL \
       AND n.\"read_at\" IS NULL AND n.\"receiver_id\" = $1 \
       AND n.\"snoozed_till\" IS NULL AND w.\"slug\" = $2",
    [
        ("total_unread_notifications_count", Int8),
        ("mention_unread_notifications_count", Int8),
    ]
}

plane_query! {
    /// `UserNotificationPreference.objects.get(user=request.user)`
    /// (`plane/app/views/notification/base.py:302`).
    ///
    /// `get()` is `LIMIT 21`; two rows raise `MultipleObjectsReturned`, which is
    /// **not** an `ObjectDoesNotExist` and therefore reaches Plane's generic
    /// 500 (`plane/app/views/base.py:235-239`). `LIMIT 2` is enough to tell the
    /// three cases apart.
    ///
    /// Bind: `$1` user id.
    USER_NOTIFICATION_PREFERENCE = "notifications_user_notification_preference",
    "SELECT u.\"created_at\", u.\"updated_at\", u.\"created_by_id\", \
     u.\"updated_by_id\", u.\"deleted_at\", u.\"id\", u.\"user_id\", \
     u.\"workspace_id\", u.\"project_id\", u.\"property_change\", \
     u.\"state_change\", u.\"comment\", u.\"mention\", u.\"issue_completed\" \
     FROM \"user_notification_preferences\" u \
     WHERE u.\"deleted_at\" IS NULL AND u.\"user_id\" = $1 LIMIT 2",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("user_id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("property_change", Bool),
        ("state_change", Bool),
        ("comment", Bool),
        ("mention", Bool),
        ("issue_completed", Bool),
    ]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(workspace__slug=…, member=…, role__lt=15,
    /// is_active=True).exists()` — the `type=created` guest branch
    /// (`plane/app/views/notification/base.py:127-129`).
    ///
    /// `role__lt=15` matches GUEST (5) only; an admin/member gets the issue-id
    /// subquery instead.
    ///
    /// Bind order: `$1` member id, `$2` workspace slug.
    WORKSPACE_GUEST_EXISTS = "notifications_workspace_guest_exists",
    "SELECT 1 AS \"exists\" FROM \"workspace_members\" wm \
     INNER JOIN \"workspaces\" w ON (wm.\"workspace_id\" = w.\"id\") \
     WHERE wm.\"deleted_at\" IS NULL AND wm.\"is_active\" \
       AND wm.\"member_id\" = $1 AND wm.\"role\" < 15 AND w.\"slug\" = $2 \
     LIMIT 1",
    [("exists", Int4)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &UNREAD_COUNTS,
    &USER_NOTIFICATION_PREFERENCE,
    &WORKSPACE_GUEST_EXISTS,
];
