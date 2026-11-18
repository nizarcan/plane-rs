// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL of `/api/instances/`.
//!
//! ## Configurations
//!
//! | Statement | Python |
//! | --- | --- |
//! | [`ALL_CONFIGURATIONS`] | `InstanceConfiguration.objects.all()` (`configuration.py:36`) |
//! | [`CONFIGURATIONS_BY_KEY`] | `.filter(key__in=request.data.keys())` (`configuration.py:46`) |
//! | [`UPDATE_CONFIGURATION_VALUES`] | `.bulk_update(values, ["value"], batch_size=100)` (`configuration.py:56`) |
//! | [`DISABLE_EMAIL_CONFIGURATION`] | `filter(key__in=…).update(value=Case(…))` (`configuration.py:68-74`) |
//!
//! `InstanceConfiguration.objects` is `SoftDeletionManager`
//! (`plane/db/mixins.py:57`), so the two reads carry `deleted_at IS NULL`;
//! `bulk_update` filters only by primary key, so the write does **not**.
//! `Meta.ordering = ("-created_at",)` (`plane/license/models/instance.py:83`)
//! is the order of both reads, and therefore of the response body.
//!
//! The `bulk_update` write is one statement for the whole set rather than
//! Django's batched `CASE WHEN`, because only the resulting rows are
//! observable: it writes `value` and nothing else — no `updated_at`, no audit
//! column, no `save()` hook, and no insert for a key that does not exist yet
//! (`PUBLIC_INSTANCES.md` §2d).
//!
//! ## The rest of the group
//!
//! | Statement | Python |
//! | --- | --- |
//! | [`INSTANCE_FIRST`] | `Instance.objects.first()` (`instance.py:41`, `admin.py:41,89,205,268,422`, `instance.py:161`) |
//! | [`INSTANCE_BY_ID_FOR_UPDATE`] | `Instance.objects.select_for_update().get(pk=…)` (`admin.py:230`) |
//! | [`INSTANCE_SAVE`] | `instance.save()` full-row write (`plane/db/models/base.py:23-45`) |
//! | [`INSTANCE_ADMINS`] | `InstanceAdmin.objects.filter(instance=instance)` (`admin.py:57`) |
//! | [`INSTANCE_ADMIN_EXISTS_FOR_USER`] | `InstanceAdmin.objects.filter(user=request.user).exists()` (`admin.py:377`) |
//! | [`INSTANCE_ADMIN_EXISTS`] | `InstanceAdmin.objects.exists()` (`admin.py:228`) |
//! | [`INSTANCE_ADMIN_BY_INSTANCE_USER`] | `InstanceAdmin.objects.filter(instance=instance, user=user)` (`admin.py:355`) |
//! | [`INSTANCE_ADMIN_CREATE`] | `InstanceAdmin.objects.create(...)` (`admin.py:59,247`) |
//! | [`INSTANCE_ADMIN_SOFT_DELETE`] | `InstanceAdmin.objects.filter(...).delete()` — `QuerySet.delete` is `update(deleted_at=now)` (`plane/db/mixins.py:48-53`) |
//! | [`USER_EXISTS_BY_EMAIL`] | `User.objects.filter(email=email).exists()` (`admin.py:213`) |
//! | [`USERS_ADMIN_LITE_BY_IDS`] | `InstanceAdminSerializer.user_detail` and `InstanceAdminMeSerializer` (`license/api/serializers/admin.py`) |
//! | [`WORKSPACE_COUNT`] | `Workspace.objects.count()` (`instance.py:153`) |
//! | [`WORKSPACE_SLUG_IEXACT_EXISTS`] | `Workspace.objects.filter(slug__iexact=slug).exists()` (`workspace.py:31`) |
//! | [`WORKSPACES_PAGE`] | `InstanceWorkSpaceEndpoint.get`'s annotated queryset (`workspace.py:52-69`) |
//! | [`WORKSPACES_COUNT`] | the same queryset's `.count()` (`plane/utils/paginator.py:175`) |
//!
//! Every read carries the manager predicate (`deleted_at IS NULL`,
//! `plane/db/mixins.py:57`); the two `QuerySet.delete`/`update` statements do
//! not, exactly like Django's.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `InstanceConfiguration.objects.all()` (`plane/license/api/views/configuration.py:36`).
    ///
    /// No binds.
    ALL_CONFIGURATIONS = "instances_all_configurations",
    "SELECT c.\"created_at\", c.\"updated_at\", c.\"created_by_id\", \
     c.\"updated_by_id\", c.\"deleted_at\", c.\"id\", c.\"key\", c.\"value\", \
     c.\"category\", c.\"is_encrypted\" \
     FROM instance_configurations c \
     WHERE c.\"deleted_at\" IS NULL \
     ORDER BY c.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("key", Varchar),
        ("value", Text),
        ("category", Text),
        ("is_encrypted", Bool),
    ]
}

plane_query! {
    /// `InstanceConfiguration.objects.filter(key__in=request.data.keys())`
    /// (`plane/license/api/views/configuration.py:46`).
    ///
    /// Django renders `key IN (%s, …)`; `key = ANY($1)` is the same predicate
    /// with one bind. An empty body is `EmptyResultSet` in Django — the caller
    /// must skip the statement rather than send an empty array
    /// (`InstanceConfigurationEndpoint.patch` does the same through
    /// `bulk_update([])` returning 0).
    ///
    /// Bind order: `$1` the keys, `text[]`.
    CONFIGURATIONS_BY_KEY = "instances_configurations_by_key",
    "SELECT c.\"created_at\", c.\"updated_at\", c.\"created_by_id\", \
     c.\"updated_by_id\", c.\"deleted_at\", c.\"id\", c.\"key\", c.\"value\", \
     c.\"category\", c.\"is_encrypted\" \
     FROM instance_configurations c \
     WHERE c.\"deleted_at\" IS NULL AND c.\"key\" = ANY($1::text[]) \
     ORDER BY c.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("key", Varchar),
        ("value", Text),
        ("category", Text),
        ("is_encrypted", Bool),
    ]
}

plane_query! {
    /// `InstanceConfiguration.objects.bulk_update(values, ["value"], batch_size=100)`
    /// (`plane/license/api/views/configuration.py:56`).
    ///
    /// Django builds
    /// `UPDATE … SET value = CASE WHEN id = %s THEN %s::text … ELSE value END WHERE id IN (…)`;
    /// setting the same column from an `unnest`ed pair list is observably the
    /// same write. Neither form touches `updated_at`, `updated_by_id` or any
    /// other column — `bulk_update` bypasses `save()` — and neither inserts a
    /// row for an unknown key.
    ///
    /// The two arrays must be the same length and in the same order. Bind
    /// order: `$1` ids (`uuid[]`), `$2` values (`text[]`, `NULL` allowed).
    UPDATE_CONFIGURATION_VALUES = "instances_update_configuration_values",
    "UPDATE instance_configurations c SET \"value\" = v.\"value\" \
     FROM (SELECT unnest($1::uuid[]) AS \"id\", unnest($2::text[]) AS \"value\") v \
     WHERE c.\"id\" = v.\"id\"",
    []
}

plane_query! {
    /// `InstanceConfiguration.objects.filter(Q(key__in=[…])).update(value=Case(
    /// When(key="ENABLE_SMTP", then=Value("0")), default=Value("")))`
    /// (`plane/license/api/views/configuration.py:68-74`).
    ///
    /// `QuerySet.update` writes exactly one column: no `updated_at`, no audit
    /// column, no save hook. The manager predicate is part of the filtered
    /// queryset, so `deleted_at IS NULL` participates.
    DISABLE_EMAIL_CONFIGURATION = "instances_disable_email_configuration",
    "UPDATE instance_configurations c SET \"value\" = \
     CASE WHEN c.\"key\" = 'ENABLE_SMTP' THEN '0' ELSE '' END \
     WHERE c.\"deleted_at\" IS NULL AND c.\"key\" = ANY($1::text[])",
    []
}

plane_query! {
    /// `Instance.objects.first()`: the soft-deletion manager predicate and
    /// `Meta.ordering = ("-created_at",)` (`plane/license/models/instance.py:52`)
    /// with `LIMIT 1`.
    INSTANCE_FIRST = "instances_first",
    "SELECT i.\"created_at\", i.\"updated_at\", i.\"created_by_id\", \
     i.\"updated_by_id\", i.\"deleted_at\", i.\"id\", i.\"instance_name\", \
     i.\"whitelist_emails\", i.\"instance_id\", i.\"current_version\", \
     i.\"latest_version\", i.\"edition\", i.\"domain\", i.\"last_checked_at\", \
     i.\"namespace\", i.\"is_telemetry_enabled\", i.\"is_support_required\", \
     i.\"is_setup_done\", i.\"is_signup_screen_visited\", i.\"is_verified\", \
     i.\"is_test\", i.\"is_current_version_deprecated\" \
     FROM instances i \
     WHERE i.\"deleted_at\" IS NULL \
     ORDER BY i.\"created_at\" DESC \
     LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("instance_name", Varchar),
        ("whitelist_emails", Text),
        ("instance_id", Varchar),
        ("current_version", Varchar),
        ("latest_version", Varchar),
        ("edition", Varchar),
        ("domain", Text),
        ("last_checked_at", Timestamptz),
        ("namespace", Varchar),
        ("is_telemetry_enabled", Bool),
        ("is_support_required", Bool),
        ("is_setup_done", Bool),
        ("is_signup_screen_visited", Bool),
        ("is_verified", Bool),
        ("is_test", Bool),
        ("is_current_version_deprecated", Bool),
    ]
}

plane_query! {
    /// `Instance.objects.select_for_update().get(pk=…)` (`admin.py:230`).
    ///
    /// `select_for_update()` carries the manager predicate and the ordering;
    /// `get` turns it into a `LIMIT 21` query in Django, but only the row is
    /// observable. Bind order: `$1` the pk.
    INSTANCE_BY_ID_FOR_UPDATE = "instances_by_id_for_update",
    "SELECT i.\"created_at\", i.\"updated_at\", i.\"created_by_id\", \
     i.\"updated_by_id\", i.\"deleted_at\", i.\"id\", i.\"instance_name\", \
     i.\"whitelist_emails\", i.\"instance_id\", i.\"current_version\", \
     i.\"latest_version\", i.\"edition\", i.\"domain\", i.\"last_checked_at\", \
     i.\"namespace\", i.\"is_telemetry_enabled\", i.\"is_support_required\", \
     i.\"is_setup_done\", i.\"is_signup_screen_visited\", i.\"is_verified\", \
     i.\"is_test\", i.\"is_current_version_deprecated\" \
     FROM instances i \
     WHERE i.\"deleted_at\" IS NULL AND i.\"id\" = $1 \
     ORDER BY i.\"created_at\" DESC \
     LIMIT 1 \
     FOR UPDATE",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("instance_name", Varchar),
        ("whitelist_emails", Text),
        ("instance_id", Varchar),
        ("current_version", Varchar),
        ("latest_version", Varchar),
        ("edition", Varchar),
        ("domain", Text),
        ("last_checked_at", Timestamptz),
        ("namespace", Varchar),
        ("is_telemetry_enabled", Bool),
        ("is_support_required", Bool),
        ("is_setup_done", Bool),
        ("is_signup_screen_visited", Bool),
        ("is_verified", Bool),
        ("is_test", Bool),
        ("is_current_version_deprecated", Bool),
    ]
}

plane_query! {
    /// `instance.save()` with no `update_fields`: Django writes every non-pk
    /// column with its in-memory value (`django/db/models/base.py:925-1008`),
    /// so only the values that changed are observable.
    ///
    /// Bind order: [`INSTANCE_FIRST`]'s columns without `id`, then `id`.
    INSTANCE_SAVE = "instances_save",
    "UPDATE instances SET \
     \"created_at\" = $1, \"updated_at\" = $2, \"created_by_id\" = $3, \
     \"updated_by_id\" = $4, \"deleted_at\" = $5, \"instance_name\" = $6, \
     \"whitelist_emails\" = $7, \"instance_id\" = $8, \
     \"current_version\" = $9, \"latest_version\" = $10, \"edition\" = $11, \
     \"domain\" = $12, \"last_checked_at\" = $13, \"namespace\" = $14, \
     \"is_telemetry_enabled\" = $15, \"is_support_required\" = $16, \
     \"is_setup_done\" = $17, \"is_signup_screen_visited\" = $18, \
     \"is_verified\" = $19, \"is_test\" = $20, \
     \"is_current_version_deprecated\" = $21 \
     WHERE \"id\" = $22",
    []
}

plane_query! {
    /// `InstanceAdmin.objects.filter(instance=instance)`: manager predicate,
    /// the FK filter and `Meta.ordering = ("-created_at",)`
    /// (`plane/license/models/instance.py:101`). Bind order: `$1` the instance.
    INSTANCE_ADMINS = "instances_admins",
    "SELECT a.\"created_at\", a.\"updated_at\", a.\"created_by_id\", \
     a.\"updated_by_id\", a.\"deleted_at\", a.\"id\", a.\"user_id\", \
     a.\"instance_id\", a.\"role\", a.\"is_verified\" \
     FROM instance_admins a \
     WHERE a.\"deleted_at\" IS NULL AND a.\"instance_id\" = $1 \
     ORDER BY a.\"created_at\" DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("user_id", Uuid),
        ("instance_id", Uuid),
        ("role", Int4),
        ("is_verified", Bool),
    ]
}

plane_query! {
    /// `InstanceAdmin.objects.filter(user=request.user).exists()`
    /// (`admin.py:377`). Bind order: `$1` the user.
    INSTANCE_ADMIN_EXISTS_FOR_USER = "instances_admin_exists_for_user",
    "SELECT EXISTS(SELECT 1 FROM instance_admins a \
     WHERE a.\"deleted_at\" IS NULL AND a.\"user_id\" = $1)",
    [("exists", Bool)]
}

plane_query! {
    /// `InstanceAdmin.objects.exists()` — the global re-check of the sign-up
    /// lock (`admin.py:228,245`).
    INSTANCE_ADMIN_EXISTS = "instances_admin_exists",
    "SELECT EXISTS(SELECT 1 FROM instance_admins a WHERE a.\"deleted_at\" IS NULL)",
    [("exists", Bool)]
}

plane_query! {
    /// `if not InstanceAdmin.objects.filter(instance=instance, user=user):`
    /// (`admin.py:355`) — a truthiness test, i.e. `EXISTS`.
    ///
    /// Bind order: `$1` the instance, `$2` the user.
    INSTANCE_ADMIN_BY_INSTANCE_USER = "instances_admin_by_instance_user",
    "SELECT EXISTS(SELECT 1 FROM instance_admins a \
     WHERE a.\"deleted_at\" IS NULL AND a.\"instance_id\" = $1 \
     AND a.\"user_id\" = $2)",
    [("exists", Bool)]
}

plane_query! {
    /// `InstanceAdmin.objects.create(instance=instance, user=user, role=role)`
    /// (`admin.py:59,247`) with `BaseModel.save`'s audit stamps
    /// (`plane/db/models/base.py:23-45`).
    ///
    /// Bind order: created_at, updated_at, created_by_id, updated_by_id,
    /// deleted_at, id, user_id, instance_id, role, is_verified.
    INSTANCE_ADMIN_CREATE = "instances_admin_create",
    "INSERT INTO instance_admins (\
     created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
     user_id, instance_id, role, is_verified) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    []
}

plane_query! {
    /// `InstanceAdmin.objects.filter(instance=instance, pk=pk).delete()`:
    /// `SoftDeletionQuerySet.delete` is `update(deleted_at=timezone.now())`
    /// (`plane/db/mixins.py:48-53`), so no other column moves.
    ///
    /// Bind order: `$1` deleted_at, `$2` the instance, `$3` the pk.
    INSTANCE_ADMIN_SOFT_DELETE = "instances_admin_soft_delete",
    "UPDATE instance_admins SET \"deleted_at\" = $1 \
     WHERE \"deleted_at\" IS NULL AND \"instance_id\" = $2 AND \"id\" = $3",
    []
}

plane_query! {
    /// `User.objects.filter(email=email).exists()` (`admin.py:213`).
    /// Bind order: `$1` the email.
    USER_EXISTS_BY_EMAIL = "instances_user_exists_by_email",
    "SELECT EXISTS(SELECT 1 FROM users u WHERE u.\"email\" = $1)",
    [("exists", Bool)]
}

plane_query! {
    /// The user half of `InstanceAdminSerializer.user_detail`
    /// (`plane/app/serializers/user.py:170-184`): the nine rendered columns
    /// plus the avatar asset's `entity_type`, in one query instead of Django's
    /// one-per-row `user` + `avatar_asset` reads.
    ///
    /// A forward FK is read through `_base_manager`, so `file_assets` is not
    /// filtered on `deleted_at`. Bind order: `$1` the user ids.
    USERS_ADMIN_LITE_BY_IDS = "instances_users_admin_lite_by_ids",
    "SELECT u.\"id\", u.\"first_name\", u.\"last_name\", u.\"avatar\", \
     u.\"is_bot\", u.\"display_name\", u.\"email\", u.\"last_login_medium\", \
     u.\"avatar_asset_id\", fa.\"entity_type\" \
     FROM users u \
     LEFT JOIN file_assets fa ON fa.\"id\" = u.\"avatar_asset_id\" \
     WHERE u.\"id\" = ANY($1::uuid[])",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("is_bot", Bool),
        ("display_name", Varchar),
        ("email", Varchar),
        ("last_login_medium", Varchar),
        ("avatar_asset_id", Uuid),
        ("entity_type", Varchar),
    ]
}

plane_query! {
    /// `Workspace.objects.count()` (`instance.py:153`): the manager predicate
    /// only, annotations are not counted.
    WORKSPACE_COUNT = "instances_workspace_count",
    "SELECT COUNT(*) FROM workspaces w WHERE w.\"deleted_at\" IS NULL",
    [("count", Int8)]
}

plane_query! {
    /// `Workspace.objects.filter(slug__iexact=slug).exists()`
    /// (`workspace.py:31`). Django renders `UPPER(slug::text) = UPPER(%s)`.
    /// Bind order: `$1` the slug.
    WORKSPACE_SLUG_IEXACT_EXISTS = "instances_workspace_slug_iexact_exists",
    "SELECT EXISTS(SELECT 1 FROM workspaces w \
     WHERE w.\"deleted_at\" IS NULL AND UPPER(w.\"slug\"::text) = UPPER($1))",
    [("exists", Bool)]
}

plane_query! {
    /// `InstanceWorkSpaceEndpoint.get`'s page (`workspace.py:52-69`):
    /// `Workspace.objects.annotate(total_projects=…, total_members=…)`, the
    /// optional `name__icontains` filter and `Meta.ordering`.
    ///
    /// The two annotations are the view's own subqueries — `Project.objects`
    /// and `WorkspaceMember.objects` both carry the manager predicate, and the
    /// member one joins `users` for `member__is_bot=False`. The logo asset's
    /// `entity_type` is joined here because `Workspace.logo_url` reads it one
    /// query per row in Django (`plane/db/models/workspace.py:145-154`).
    ///
    /// `$1` is `NULL` when there is no search, else the escaped
    /// `%…%` pattern; `$2`/`$3` are LIMIT/OFFSET.
    WORKSPACES_PAGE = "instances_workspaces_page",
    "SELECT w.\"created_at\", w.\"updated_at\", w.\"created_by_id\", \
     w.\"updated_by_id\", w.\"deleted_at\", w.\"id\", w.\"name\", w.\"logo\", \
     w.\"logo_asset_id\", w.\"owner_id\", w.\"slug\", w.\"organization_size\", \
     w.\"timezone\", w.\"background_color\", \
     (SELECT COUNT(*) FROM projects p \
      WHERE p.\"workspace_id\" = w.\"id\" AND p.\"deleted_at\" IS NULL) \
      AS total_projects, \
     (SELECT COUNT(*) FROM workspace_members wm \
      INNER JOIN users mu ON mu.\"id\" = wm.\"member_id\" \
      WHERE wm.\"workspace_id\" = w.\"id\" AND wm.\"deleted_at\" IS NULL \
      AND mu.\"is_bot\" = FALSE AND wm.\"is_active\" = TRUE) \
      AS total_members, \
     la.\"entity_type\" \
     FROM workspaces w \
     LEFT JOIN file_assets la ON la.\"id\" = w.\"logo_asset_id\" \
     WHERE w.\"deleted_at\" IS NULL \
     AND ($1::text IS NULL OR UPPER(w.\"name\"::text) LIKE UPPER($1::text)) \
     ORDER BY w.\"created_at\" DESC \
     LIMIT $2 OFFSET $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("logo", Text),
        ("logo_asset_id", Uuid),
        ("owner_id", Uuid),
        ("slug", Varchar),
        ("organization_size", Varchar),
        ("timezone", Varchar),
        ("background_color", Varchar),
        ("total_projects", Int8),
        ("total_members", Int8),
        ("entity_type", Varchar),
    ]
}

plane_query! {
    /// The `.count()` of the same queryset (`plane/utils/paginator.py:175`):
    /// Django drops the annotations from a count.
    ///
    /// `$1` is `NULL` when there is no search, else the escaped pattern.
    WORKSPACES_COUNT = "instances_workspaces_count",
    "SELECT COUNT(*) FROM workspaces w \
     WHERE w.\"deleted_at\" IS NULL \
     AND ($1::text IS NULL OR UPPER(w.\"name\"::text) LIKE UPPER($1::text))",
    [("count", Int8)]
}

plane_query! {
    /// `Workspace.objects.filter(slug__iexact=value).exists()` for the
    /// serializer's `validate_slug` (`license/api/serializers/workspace.py:35-37`).
    ///
    /// Same predicate as [`WORKSPACE_SLUG_IEXACT_EXISTS`]; kept separate so the
    /// two call sites can diverge with their Python.
    WORKSPACE_SLUG_IN_USE = "instances_workspace_slug_in_use",
    "SELECT EXISTS(SELECT 1 FROM workspaces w \
     WHERE w.\"deleted_at\" IS NULL AND UPPER(w.\"slug\"::text) = UPPER($1))",
    [("exists", Bool)]
}

plane_query! {
    /// `FileAsset.asset_url`'s only input for the static entity types
    /// (`plane/db/models/asset.py:83-104`), used by `User.avatar_url`
    /// (`plane/db/models/user.py:143-151`).
    ///
    /// A forward FK is read through `_base_manager`, so no `deleted_at` filter.
    /// Bind order: `$1` the asset id.
    ASSET_ENTITY_TYPE = "instances_asset_entity_type",
    "SELECT fa.\"entity_type\" FROM file_assets fa WHERE fa.\"id\" = $1",
    [("entity_type", Varchar)]
}

/// Every statement added by the instances handlers, for
/// `tests/schema_check.rs`.
pub const ALL_QUERIES: &[&Query] = &[
    &ALL_CONFIGURATIONS,
    &CONFIGURATIONS_BY_KEY,
    &UPDATE_CONFIGURATION_VALUES,
    &DISABLE_EMAIL_CONFIGURATION,
    &INSTANCE_FIRST,
    &INSTANCE_BY_ID_FOR_UPDATE,
    &INSTANCE_SAVE,
    &INSTANCE_ADMINS,
    &INSTANCE_ADMIN_EXISTS_FOR_USER,
    &INSTANCE_ADMIN_EXISTS,
    &INSTANCE_ADMIN_BY_INSTANCE_USER,
    &INSTANCE_ADMIN_CREATE,
    &INSTANCE_ADMIN_SOFT_DELETE,
    &USER_EXISTS_BY_EMAIL,
    &USERS_ADMIN_LITE_BY_IDS,
    &WORKSPACE_COUNT,
    &WORKSPACE_SLUG_IEXACT_EXISTS,
    &WORKSPACES_PAGE,
    &WORKSPACES_COUNT,
    &WORKSPACE_SLUG_IN_USE,
    &ASSET_ENTITY_TYPE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_are_unique() {
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }
}
