// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The statements of workspace-core set C (`plans/WORKSPACES_CORE.md` §3):
//! routes 207, 218, 219, 220, 221 and 225.
//!
//! `workspaces/write_queries.rs` (the P0's file) holds what the three sets
//! share. This file holds what only set C runs: the lazily created properties
//! row and its full save, the quick-link duplicate check / insert / full save,
//! the two preference `.first()` lookups with their saves, and 225's
//! two-column update.
//!
//! Every predicate is Django's, from the compiled queryset. Two rules recur:
//!
//! * the default manager's `deleted_at IS NULL` applies to the model the view
//!   queried, never to a model it merely joined (`plane/db/mixins.py:56-58`),
//!   so a preference or quick link of a soft-deleted workspace still resolves
//!   through the `workspaces` join;
//! * `Meta.ordering` is part of `.first()`: `workspace_home_preferences`,
//!   `workspace_user_preferences` and `workspace_user_links` declare
//!   `-created_at`, so the lookup carries `ORDER BY created_at DESC LIMIT 1`
//!   (`django/db/models/query.py:first`).
//!
//! A `save()` writes **every** concrete column (Django's `_do_update` builds
//! the SET list from `_meta.local_concrete_fields`, not from the changed
//! attributes), which is why the UPDATEs below bind columns the request never
//! touches. The one exception is 225: `save(update_fields=["is_pinned",
//! "sort_order"])` writes exactly those two (`user_preference.py:101`).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `WorkspaceUserProperties.objects.get_or_create(user=request.user,
    /// workspace_id=workspace.id)`'s **create** half
    /// (`plane/app/views/workspace/user.py:277-279`, `:291-293`), with the
    /// model defaults (`plane/db/models/workspace.py:352-390`).
    ///
    /// `get_or_create` runs its `create()` in a transaction and retries the
    /// `get()` on an `IntegrityError`; `ON CONFLICT DO NOTHING` is the same
    /// outcome in one statement. The caller re-reads when nothing was
    /// returned.
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` id, `$5` workspace, `$6` user, `$7` filters,
    /// `$8` display_filters, `$9` display_properties.
    USER_PROPERTIES_INSERT = "workspaces_c_user_properties_insert",
    "INSERT INTO workspace_user_properties \
       (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
        workspace_id, user_id, filters, display_filters, display_properties, \
        rich_filters, navigation_project_limit, navigation_control_preference) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9, '{}'::jsonb, 10, \
       'ACCORDION') \
     ON CONFLICT DO NOTHING \
     RETURNING created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
       workspace_id, user_id, filters, display_filters, display_properties, \
       rich_filters, navigation_project_limit, navigation_control_preference",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("filters", Jsonb),
        ("display_filters", Jsonb),
        ("display_properties", Jsonb),
        ("rich_filters", Jsonb),
        ("navigation_project_limit", Int4),
        ("navigation_control_preference", Varchar),
    ]
}

plane_query! {
    /// `WorkspaceUserProperties.save()` on the existing row — 207's
    /// `serializer.save()` (`plane/app/views/workspace/user.py:283`,
    /// `rest_framework/serializers.py:960-988`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by, `$5` deleted_at, `$6` filters, `$7` display_filters,
    /// `$8` display_properties, `$9` rich_filters,
    /// `$10` navigation_project_limit, `$11` navigation_control_preference,
    /// `$12` workspace, `$13` user, `$14` id.
    USER_PROPERTIES_SAVE = "workspaces_c_user_properties_save",
    "UPDATE workspace_user_properties SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, filters = $6, \
       display_filters = $7, display_properties = $8, rich_filters = $9, \
       navigation_project_limit = $10, navigation_control_preference = $11, \
       workspace_id = $12, user_id = $13 \
     WHERE id = $14",
    []
}

plane_query! {
    /// `WorkspaceUserLink.objects.filter(url=url, workspace_id=…, owner_id=…)
    /// .exists()` — `WorkspaceUserLinkSerializer.create` (`serializers/
    /// workspace.py:210-224`) — and the same queryset with `.exclude(pk=…)`
    /// in `update` (`:226-238`).
    ///
    /// `WorkspaceUserLink.objects` is the soft-deletion manager, so a
    /// soft-deleted link does not block a new one. The exclusion is folded in
    /// so both call sites share one prepared shape: `$2` is the instance pk on
    /// an update and NULL on a create, where `($2::uuid IS NULL OR id <> $2)`
    /// is always true.
    ///
    /// Bind order: `$1` url, `$2` workspace id, `$3` owner id, `$4` excluded
    /// pk or NULL.
    QUICK_LINK_URL_TAKEN = "workspaces_c_quick_link_url_taken",
    "SELECT EXISTS ( \
       SELECT 1 FROM workspace_user_links \
       WHERE deleted_at IS NULL AND url = $1 AND workspace_id = $2 \
         AND owner_id = $3 AND ($4::uuid IS NULL OR id <> $4) \
     ) AS taken",
    [("taken", Bool)]
}

plane_query! {
    /// `WorkspaceUserLinkSerializer.save(workspace_id=…, owner_id=…)` on a
    /// create (`plane/app/views/workspace/quick_link.py:25-33`):
    /// `Model.objects.create` with the validated fields plus the two kwargs,
    /// the `metadata` default `{}` and H01's audit columns. `workspace_id` is
    /// the project's workspace when the body carried one
    /// (`WorkspaceBaseModel.save`, `plane/db/models/workspace.py:192-195`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by (NULL on a create), `$5` deleted_at, `$6` id,
    /// `$7` workspace, `$8` project, `$9` title, `$10` url, `$11` metadata,
    /// `$12` owner.
    QUICK_LINK_INSERT = "workspaces_c_quick_link_insert",
    "INSERT INTO workspace_user_links \
       (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
        workspace_id, project_id, title, url, metadata, owner_id) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    []
}

plane_query! {
    /// `WorkspaceUserLink.save()` on an existing row — 219's
    /// `serializer.save()` (`plane/app/views/workspace/quick_link.py:34-44`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by, `$5` deleted_at, `$6` workspace, `$7` project,
    /// `$8` title, `$9` url, `$10` metadata, `$11` owner, `$12` id.
    QUICK_LINK_SAVE = "workspaces_c_quick_link_save",
    "UPDATE workspace_user_links SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, \
       workspace_id = $6, project_id = $7, title = $8, url = $9, \
       metadata = $10, owner_id = $11 \
     WHERE id = $12",
    []
}

plane_query! {
    /// `WorkspaceHomePreference.objects.filter(key=key, workspace__slug=slug,
    /// user=request.user).first()` — 221's lookup
    /// (`plane/app/views/workspace/home.py:68-70`).
    ///
    /// `.first()` is `ORDER BY created_at DESC LIMIT 1` on the model's default
    /// ordering (`plane/db/models/workspace.py:398`). The `workspaces` join has
    /// no `deleted_at` predicate: only the queried model's manager applies.
    ///
    /// Bind order: `$1` key, `$2` slug, `$3` user id.
    HOME_PREFERENCE_BY_KEY = "workspaces_c_home_preference_by_key",
    "SELECT h.created_at, h.updated_at, h.created_by_id, h.updated_by_id, \
       h.deleted_at, h.id, h.workspace_id, h.user_id, h.key, h.is_enabled, \
       h.config, h.sort_order \
     FROM workspace_home_preferences h \
     INNER JOIN workspaces w ON w.id = h.workspace_id \
     WHERE h.deleted_at IS NULL AND h.key = $1 AND w.slug = $2 AND h.user_id = $3 \
     ORDER BY h.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("key", Varchar),
        ("is_enabled", Bool),
        ("config", Jsonb),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `WorkspaceHomePreference.save()` — 221's `serializer.save()`
    /// (`plane/app/views/workspace/home.py:72-77`).
    ///
    /// Bind order: `$1` created_at, `$2` updated_at, `$3` created_by,
    /// `$4` updated_by, `$5` deleted_at, `$6` workspace, `$7` user, `$8` key,
    /// `$9` is_enabled, `$10` config, `$11` sort_order, `$12` id.
    HOME_PREFERENCE_SAVE = "workspaces_c_home_preference_save",
    "UPDATE workspace_home_preferences SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, \
       workspace_id = $6, user_id = $7, key = $8, is_enabled = $9, \
       config = $10, sort_order = $11 \
     WHERE id = $12",
    []
}

plane_query! {
    /// `WorkspaceUserPreference.objects.filter(key=key, workspace__slug=slug,
    /// user=request.user).first()` — 225's per-element lookup
    /// (`plane/app/views/workspace/user_preference.py:85-89`).
    ///
    /// Bind order: `$1` key, `$2` slug, `$3` user id.
    USER_PREFERENCE_BY_KEY = "workspaces_c_user_preference_by_key",
    "SELECT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
       p.deleted_at, p.id, p.workspace_id, p.user_id, p.key, p.is_pinned, \
       p.sort_order \
     FROM workspace_user_preferences p \
     INNER JOIN workspaces w ON w.id = p.workspace_id \
     WHERE p.deleted_at IS NULL AND p.key = $1 AND w.slug = $2 AND p.user_id = $3 \
     ORDER BY p.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("user_id", Uuid),
        ("key", Varchar),
        ("is_pinned", Bool),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `preference.save(update_fields=["is_pinned", "sort_order"])`
    /// (`plane/app/views/workspace/user_preference.py:101`).
    ///
    /// The only two columns `_do_update` keeps, so `updated_at`,
    /// `updated_by_id` and every other column keep their stored values — the
    /// H01 assignment the model's `save()` makes is never written.
    ///
    /// Bind order: `$1` is_pinned, `$2` sort_order, `$3` id.
    USER_PREFERENCE_UPDATE = "workspaces_c_user_preference_update",
    "UPDATE workspace_user_preferences SET is_pinned = $1, sort_order = $2 \
     WHERE id = $3",
    []
}

/// Every catalogued statement of this module, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &USER_PROPERTIES_INSERT,
    &USER_PROPERTIES_SAVE,
    &QUICK_LINK_URL_TAKEN,
    &QUICK_LINK_INSERT,
    &QUICK_LINK_SAVE,
    &HOME_PREFERENCE_BY_KEY,
    &HOME_PREFERENCE_SAVE,
    &USER_PREFERENCE_BY_KEY,
    &USER_PREFERENCE_UPDATE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_c_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(
            declared,
            ALL_QUERIES.len(),
            "write_c_queries.rs declares {declared} statements but ALL_QUERIES has {}",
            ALL_QUERIES.len()
        );
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
