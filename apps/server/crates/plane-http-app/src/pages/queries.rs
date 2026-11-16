// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `pages` group.
//!
//! Reproduces the ORM statements of
//!
//! * `plane/app/views/page/base.py` — `PageViewSet` (`get_queryset`, `list`,
//!   `retrieve`, `summary`);
//! * `plane/app/views/page/version.py` — `PageVersionEndpoint.get`;
//! * `plane/app/views/page/live.py` — `PageLiveAccessEndpoint.get`.
//!
//! The list statement is the one shape that is *dynamic*: `?search=` appends a
//! `LIKE` per term and `order_by` selects one of eight `ORDER BY` clauses, so
//! [`PAGES_SELECT`] + [`PAGES_GROUP_BY`] hold the fixed part and the handlers
//! append the filter and the ordering, sending the result as an unnamed
//! statement (`DECISIONS.md` D1's 2026-11-16 amendment). The default shape — no
//! search, default `order_by`, no guest filter — is catalogued as
//! [`PAGES_LIST`] and prepared once per connection; the unit test
//! `the_catalogued_list_is_the_dynamic_base_plus_the_default_order` keeps the
//! texts identical.
//!
//! `PageViewSet.get_queryset` (`base.py:83-143`) is one statement with four
//! annotations and two `ARRAY_AGG`s over the joins its filters already require:
//!
//! * `is_favorite` — `Exists(UserFavorite…)`, joined to `workspaces` for the
//!   `workspace__slug` term (`base.py:84-89,104`);
//! * `project` — `Exists(ProjectPage(page_id=OuterRef("id"),
//!   project_id=project_id))`, used only as `filter(project=True)`
//!   (`base.py:122-126,141`), so it is written as a `WHERE EXISTS`;
//! * `label_ids` / `project_ids` — `Coalesce(ArrayAgg(…, distinct=True,
//!   filter=…), Value([]))` over the `page_labels` and `project_pages` joins
//!   (`base.py:127-140`). `project_ids` aggregates over the *same*
//!   `project_pages → projects → project_members` join the `WHERE` restricts,
//!   which is why it only lists projects the caller is an active,
//!   non-archived member of (a separate join would list every linked project);
//! * `DISTINCT` + `GROUP BY pages.id, workspaces.id, users.id` — Django groups
//!   by its two `select_related` aliases as well (`base.py:102-103`). Grouping
//!   by `pages.id` alone yields the same rows because both aliases are
//!   functionally dependent on the page and neither is aggregated; the
//!   redundant `DISTINCT` is kept so the text stays Django's.
//!
//! `sort_order` is selected although no serializer field reads it: the
//! `order_by=sort_order` variant orders by a column that `SELECT DISTINCT`
//! requires in the select list (`ORDER BY expressions must appear in select
//! list`), and Django's own select list has every page column.

use plane_db::Query;
use plane_db::plane_query;

/// Everything in `PageViewSet.get_queryset` (`page/base.py:83-143`) up to the
/// grouping: select list, joins and `WHERE`.
///
/// Parameters: `$1` workspace slug, `$2` project id, `$3` user id.
pub const PAGES_SELECT: &str = "SELECT DISTINCT p.id AS id, p.name AS name, \
 p.owned_by_id AS owned_by_id, p.access AS access, p.color AS color, \
 p.parent_id AS parent_id, \
 EXISTS (SELECT 1 FROM user_favorites uf INNER JOIN workspaces ufw \
   ON uf.workspace_id = ufw.id \
   WHERE uf.deleted_at IS NULL AND uf.entity_identifier = p.id \
   AND uf.entity_type = 'page' AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
 p.is_locked AS is_locked, p.archived_at AS archived_at, \
 p.workspace_id AS workspace_id, p.created_at AS created_at, \
 p.updated_at AS updated_at, p.created_by_id AS created_by_id, \
 p.updated_by_id AS updated_by_id, p.view_props AS view_props, \
 p.logo_props AS logo_props, \
 COALESCE(ARRAY_AGG(DISTINCT pl.label_id) \
   FILTER (WHERE NOT (pl.label_id IS NULL)), '{}'::uuid[]) AS label_ids, \
 COALESCE(ARRAY_AGG(DISTINCT pp.project_id) \
   FILTER (WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001'::uuid)), \
   '{}'::uuid[]) AS project_ids, \
 p.description_html AS description_html, p.sort_order AS sort_order \
 FROM pages p \
 INNER JOIN workspaces w ON p.workspace_id = w.id \
 INNER JOIN project_pages pp ON p.id = pp.page_id \
 INNER JOIN projects pr ON pp.project_id = pr.id \
 INNER JOIN project_members pm ON pr.id = pm.project_id \
 LEFT OUTER JOIN page_labels pl ON p.id = pl.page_id \
 WHERE p.deleted_at IS NULL AND w.slug = $1 AND pr.archived_at IS NULL \
 AND pm.is_active AND pm.member_id = $3 AND p.parent_id IS NULL \
 AND (p.owned_by_id = $3 OR p.access = 0) \
 AND EXISTS (SELECT 1 FROM project_pages x WHERE x.deleted_at IS NULL \
   AND x.page_id = p.id AND x.project_id = $2)";

/// The grouping `PageViewSet.get_queryset` needs for its two `ARRAY_AGG`s.
pub const PAGES_GROUP_BY: &str = " GROUP BY p.id";

/// The default ordering: favourites first, then the sanitized user ordering,
/// then `id` as the pagination tiebreak (`base.py:113-121`).
pub const PAGES_DEFAULT_ORDER: &str = " ORDER BY is_favorite DESC, p.created_at DESC, p.id ASC";

/// The `order_by` allowlist of `PageViewSet` (`plane/utils/order_queryset.py:79-84`),
/// in the order [`order_clause`](crate::pages::base::order_clause) lists them.
pub const PAGE_ORDER_BY_ALLOWLIST: &[&str] = &["created_at", "updated_at", "name", "sort_order"];

plane_query! {
    /// `PageViewSet.get_queryset` with no `?search=`, the default `order_by` and
    /// no guest filter (`page/base.py:83-143`).
    PAGES_LIST = "pages_list",
    "SELECT DISTINCT p.id AS id, p.name AS name, \
     p.owned_by_id AS owned_by_id, p.access AS access, p.color AS color, \
     p.parent_id AS parent_id, \
     EXISTS (SELECT 1 FROM user_favorites uf INNER JOIN workspaces ufw \
       ON uf.workspace_id = ufw.id \
       WHERE uf.deleted_at IS NULL AND uf.entity_identifier = p.id \
       AND uf.entity_type = 'page' AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     p.is_locked AS is_locked, p.archived_at AS archived_at, \
     p.workspace_id AS workspace_id, p.created_at AS created_at, \
     p.updated_at AS updated_at, p.created_by_id AS created_by_id, \
     p.updated_by_id AS updated_by_id, p.view_props AS view_props, \
     p.logo_props AS logo_props, \
     COALESCE(ARRAY_AGG(DISTINCT pl.label_id) \
       FILTER (WHERE NOT (pl.label_id IS NULL)), '{}'::uuid[]) AS label_ids, \
     COALESCE(ARRAY_AGG(DISTINCT pp.project_id) \
       FILTER (WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001'::uuid)), \
       '{}'::uuid[]) AS project_ids, \
     p.description_html AS description_html, p.sort_order AS sort_order \
     FROM pages p \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN projects pr ON pp.project_id = pr.id \
     INNER JOIN project_members pm ON pr.id = pm.project_id \
     LEFT OUTER JOIN page_labels pl ON p.id = pl.page_id \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND pr.archived_at IS NULL \
     AND pm.is_active AND pm.member_id = $3 AND p.parent_id IS NULL \
     AND (p.owned_by_id = $3 OR p.access = 0) \
     AND EXISTS (SELECT 1 FROM project_pages x WHERE x.deleted_at IS NULL \
       AND x.page_id = p.id AND x.project_id = $2) \
     GROUP BY p.id \
     ORDER BY is_favorite DESC, p.created_at DESC, p.id ASC",
    [
        ("id", Uuid),
        ("name", Text),
        ("owned_by_id", Uuid),
        ("access", Int2),
        ("color", Varchar),
        ("parent_id", Uuid),
        ("is_favorite", Bool),
        ("is_locked", Bool),
        ("archived_at", Date),
        ("workspace_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("view_props", Jsonb),
        ("logo_props", Jsonb),
        ("label_ids", ArrayOf(Uuid)),
        ("project_ids", ArrayOf(Uuid)),
        ("description_html", Text),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `PageViewSet.retrieve`'s `self.get_queryset().filter(pk=page_id).first()`
    /// (`page/base.py:219`): the same statement plus the pk filter, the default
    /// ordering and `LIMIT 1`.
    ///
    /// The ordering is irrelevant to the result—`pk` is unique—but Django's
    /// `first()` keeps the queryset's `ORDER BY`, so the statement carries it.
    /// `$4` page id.
    PAGES_RETRIEVE = "pages_retrieve",
    "SELECT DISTINCT p.id AS id, p.name AS name, \
     p.owned_by_id AS owned_by_id, p.access AS access, p.color AS color, \
     p.parent_id AS parent_id, \
     EXISTS (SELECT 1 FROM user_favorites uf INNER JOIN workspaces ufw \
       ON uf.workspace_id = ufw.id \
       WHERE uf.deleted_at IS NULL AND uf.entity_identifier = p.id \
       AND uf.entity_type = 'page' AND uf.user_id = $3 AND ufw.slug = $1) AS is_favorite, \
     p.is_locked AS is_locked, p.archived_at AS archived_at, \
     p.workspace_id AS workspace_id, p.created_at AS created_at, \
     p.updated_at AS updated_at, p.created_by_id AS created_by_id, \
     p.updated_by_id AS updated_by_id, p.view_props AS view_props, \
     p.logo_props AS logo_props, \
     COALESCE(ARRAY_AGG(DISTINCT pl.label_id) \
       FILTER (WHERE NOT (pl.label_id IS NULL)), '{}'::uuid[]) AS label_ids, \
     COALESCE(ARRAY_AGG(DISTINCT pp.project_id) \
       FILTER (WHERE NOT (pp.project_id = '00000000-0000-0000-0000-000000000001'::uuid)), \
       '{}'::uuid[]) AS project_ids, \
     p.description_html AS description_html, p.sort_order AS sort_order \
     FROM pages p \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN projects pr ON pp.project_id = pr.id \
     INNER JOIN project_members pm ON pr.id = pm.project_id \
     LEFT OUTER JOIN page_labels pl ON p.id = pl.page_id \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND pr.archived_at IS NULL \
     AND pm.is_active AND pm.member_id = $3 AND p.parent_id IS NULL \
     AND (p.owned_by_id = $3 OR p.access = 0) \
     AND EXISTS (SELECT 1 FROM project_pages x WHERE x.deleted_at IS NULL \
       AND x.page_id = p.id AND x.project_id = $2) \
     AND p.id = $4 \
     GROUP BY p.id \
     ORDER BY is_favorite DESC, p.created_at DESC, p.id ASC \
     LIMIT 1",
    [
        ("id", Uuid),
        ("name", Text),
        ("owned_by_id", Uuid),
        ("access", Int2),
        ("color", Varchar),
        ("parent_id", Uuid),
        ("is_favorite", Bool),
        ("is_locked", Bool),
        ("archived_at", Date),
        ("workspace_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("view_props", Jsonb),
        ("logo_props", Jsonb),
        ("label_ids", ArrayOf(Uuid)),
        ("project_ids", ArrayOf(Uuid)),
        ("description_html", Text),
        ("sort_order", Float8),
    ]
}

plane_query! {
    /// `PageViewSet.list`/`summary`'s project row plus both predicates they read
    /// off the caller's membership (`page/base.py:288-299,435-446`).
    ///
    /// `Project.objects.get(pk=project_id)` (`base.py:288,435`) and
    /// `ProjectMember.objects.filter(…, role=5, is_active=True).exists()`
    /// (`base.py:290-296,437-443`) are separate statements in Python; they are
    /// one here because neither depends on the other and only two facts are read
    /// off them. The `EXISTS` is `role = ROLE.GUEST.value` = `5`.
    ///
    /// `$1` user id, `$2` project id, `$3` workspace slug.
    PAGES_PROJECT_GUEST = "pages_project_guest",
    "SELECT pr.guest_view_all_features AS guest_view_all_features, \
     EXISTS (SELECT 1 FROM project_members pm INNER JOIN workspaces w \
       ON pm.workspace_id = w.id \
       WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $1 \
       AND pm.project_id = $2 AND pm.role = 5 AND w.slug = $3) AS guest_member \
     FROM projects pr \
     WHERE pr.deleted_at IS NULL AND pr.id = $2 \
     LIMIT 1",
    [("guest_view_all_features", Bool), ("guest_member", Bool)]
}

plane_query! {
    /// `PageViewSet.summary`'s `queryset.aggregate(...)` (`page/base.py:448-464`).
    ///
    /// The queryset is `.distinct()`, so Django wraps it in a subquery before
    /// applying the aggregates (`django/db/models/sql/query.py`,
    /// `get_aggregation`); the inner `DISTINCT` keeps the statement's shape.
    /// Distinctness over `(id, access, archived_at)` is the same as Django's
    /// over every page column, because `id` alone is unique.
    ///
    /// `$1` workspace slug, `$2` user id, `$3` project id.
    PAGES_SUMMARY_COUNTS = "pages_summary_counts",
    "SELECT COUNT(CASE WHEN (sub.access = 0 AND sub.archived_at IS NULL) \
       THEN 1 ELSE NULL END) AS public_pages, \
     COUNT(CASE WHEN (sub.access = 1 AND sub.archived_at IS NULL) \
       THEN 1 ELSE NULL END) AS private_pages, \
     COUNT(CASE WHEN sub.archived_at IS NOT NULL THEN 1 ELSE NULL END) AS archived_pages \
     FROM (SELECT DISTINCT p.id AS id, p.access AS access, p.archived_at AS archived_at \
       FROM pages p \
       INNER JOIN workspaces w ON p.workspace_id = w.id \
       INNER JOIN project_pages pp ON p.id = pp.page_id \
       INNER JOIN projects pr ON pp.project_id = pr.id \
       INNER JOIN project_members pm ON pr.id = pm.project_id \
       WHERE p.deleted_at IS NULL AND w.slug = $1 AND pr.archived_at IS NULL \
       AND pm.is_active AND pm.member_id = $2 AND p.parent_id IS NULL \
       AND (p.owned_by_id = $2 OR p.access = 0) \
       AND EXISTS (SELECT 1 FROM project_pages x WHERE x.deleted_at IS NULL \
         AND x.page_id = p.id AND x.project_id = $3)) sub",
    [
        ("public_pages", Int8),
        ("private_pages", Int8),
        ("archived_pages", Int8),
    ]
}

plane_query! {
    /// [`PAGES_SUMMARY_COUNTS`] with `queryset.filter(owned_by=user)` applied,
    /// which is what `PageViewSet.summary` does for a guest in a project
    /// without `guest_view_all_features` (`page/base.py:446`).
    ///
    /// A separate statement because the extra `AND` sits inside the subquery;
    /// making it a format argument would put the statement on the dynamic path
    /// for no reason.
    PAGES_SUMMARY_COUNTS_GUEST = "pages_summary_counts_guest",
    "SELECT COUNT(CASE WHEN (sub.access = 0 AND sub.archived_at IS NULL) \
       THEN 1 ELSE NULL END) AS public_pages, \
     COUNT(CASE WHEN (sub.access = 1 AND sub.archived_at IS NULL) \
       THEN 1 ELSE NULL END) AS private_pages, \
     COUNT(CASE WHEN sub.archived_at IS NOT NULL THEN 1 ELSE NULL END) AS archived_pages \
     FROM (SELECT DISTINCT p.id AS id, p.access AS access, p.archived_at AS archived_at \
       FROM pages p \
       INNER JOIN workspaces w ON p.workspace_id = w.id \
       INNER JOIN project_pages pp ON p.id = pp.page_id \
       INNER JOIN projects pr ON pp.project_id = pr.id \
       INNER JOIN project_members pm ON pr.id = pm.project_id \
       WHERE p.deleted_at IS NULL AND w.slug = $1 AND pr.archived_at IS NULL \
       AND pm.is_active AND pm.member_id = $2 AND p.parent_id IS NULL \
       AND (p.owned_by_id = $2 OR p.access = 0) \
       AND EXISTS (SELECT 1 FROM project_pages x WHERE x.deleted_at IS NULL \
         AND x.page_id = p.id AND x.project_id = $3) \
       AND p.owned_by_id = $2) sub",
    [
        ("public_pages", Int8),
        ("private_pages", Int8),
        ("archived_pages", Int8),
    ]
}

plane_query! {
    /// `PageViewSet.retrieve`'s `PageLog.objects.filter(page_id=page_id,
    /// entity_name="issue").values_list("entity_identifier", flat=True)`
    /// (`page/base.py:226-228`).
    ///
    /// The default manager adds `deleted_at IS NULL` (`plane/db/mixins.py:57`)
    /// and `PageLog.Meta.ordering = ("-created_at",)` supplies the `ORDER BY`.
    /// `entity_identifier` is nullable; a NULL row renders as `null` in the
    /// appended array.
    PAGES_ISSUE_IDS = "pages_issue_ids",
    "SELECT entity_identifier AS entity_identifier FROM page_logs \
     WHERE deleted_at IS NULL AND entity_name = 'issue' AND page_id = $1 \
     ORDER BY created_at DESC",
    [("entity_identifier", Uuid)]
}

plane_query! {
    /// `PageVersionEndpoint.get`'s list branch (`page/version.py:45-50`).
    ///
    /// `PageVersion.objects.filter(workspace__slug=…, page__project_pages__project_id=…,
    /// page__project_pages__deleted_at__isnull=True, page_id=…)` with the
    /// model's `ordering = ("-created_at",)`. Note the joins: `pages` is joined
    /// but not filtered on `pages.deleted_at`, exactly as Django emits it.
    ///
    /// `$1` project id, `$2` page id, `$3` workspace slug.
    PAGES_VERSIONS = "pages_versions",
    "SELECT pv.id AS id, pv.workspace_id AS workspace_id, pv.page_id AS page_id, \
     pv.last_saved_at AS last_saved_at, pv.owned_by_id AS owned_by_id, \
     pv.created_at AS created_at, pv.updated_at AS updated_at, \
     pv.created_by_id AS created_by_id, pv.updated_by_id AS updated_by_id \
     FROM page_versions pv \
     INNER JOIN pages p ON pv.page_id = p.id \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN workspaces w ON pv.workspace_id = w.id \
     WHERE pv.deleted_at IS NULL AND pp.deleted_at IS NULL \
     AND pp.project_id = $1 AND pv.page_id = $2 AND w.slug = $3 \
     ORDER BY pv.created_at DESC",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("page_id", Uuid),
        ("last_saved_at", Timestamptz),
        ("owned_by_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ]
}

plane_query! {
    /// `PageVersionEndpoint.get`'s detail branch (`page/version.py:29-42`): the
    /// same filters plus `pk`, `.distinct()` and `.get()` (`LIMIT 21`).
    ///
    /// `$4` page version id.
    PAGES_VERSION_DETAIL = "pages_version_detail",
    "SELECT DISTINCT pv.id AS id, pv.workspace_id AS workspace_id, \
     pv.page_id AS page_id, pv.last_saved_at AS last_saved_at, \
     pv.description_binary AS description_binary, \
     pv.description_html AS description_html, \
     pv.description_json AS description_json, pv.owned_by_id AS owned_by_id, \
     pv.created_at AS created_at, pv.updated_at AS updated_at, \
     pv.created_by_id AS created_by_id, pv.updated_by_id AS updated_by_id \
     FROM page_versions pv \
     INNER JOIN pages p ON pv.page_id = p.id \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN workspaces w ON pv.workspace_id = w.id \
     WHERE pv.deleted_at IS NULL AND pp.deleted_at IS NULL \
     AND pp.project_id = $1 AND pv.page_id = $2 AND pv.id = $4 AND w.slug = $3 \
     LIMIT 21",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("page_id", Uuid),
        ("last_saved_at", Timestamptz),
        ("description_binary", Bytea),
        ("description_html", Text),
        ("description_json", Jsonb),
        ("owned_by_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ]
}

plane_query! {
    /// `PagesDescriptionViewSet.retrieve`'s `Page.objects.get(...)`
    /// (`page/base.py:499-505`), projected to the one column the view streams.
    ///
    /// `.get()` adds `LIMIT 21`; the handler checks the row count so a second
    /// active link to the same project would 404 (`MultipleObjectsReturned` is
    /// an `ObjectDoesNotExist`) instead of silently picking one.
    ///
    /// `$1` user id, `$2` page id, `$3` project id, `$4` workspace slug.
    PAGES_DESCRIPTION_BINARY = "pages_description_binary",
    "SELECT p.description_binary AS description_binary FROM pages p \
     INNER JOIN project_pages pp ON p.id = pp.page_id \
     INNER JOIN workspaces w ON p.workspace_id = w.id \
     WHERE p.deleted_at IS NULL \
     AND (p.owned_by_id = $1 OR p.access = 0) \
     AND p.id = $2 AND pp.deleted_at IS NULL AND pp.project_id = $3 \
     AND w.slug = $4 \
     LIMIT 21",
    [("description_binary", Bytea)]
}

plane_query! {
    /// `PageLiveAccessEndpoint.get`'s `ProjectPage.objects.filter(…).values_list(
    /// "project_id", "project__project_projectmember__role", "page__owned_by_id",
    /// "page__access")` (`page/live.py:35-44`).
    ///
    /// `ProjectPage.Meta.ordering = ("-created_at",)` supplies the `ORDER BY`,
    /// which decides which link wins when a page belongs to several projects.
    /// The duplicate `pp.deleted_at IS NULL` is Django's: the default manager's
    /// predicate plus `deleted_at__isnull=True` (`page/live.py:37`).
    ///
    /// `$1` page id, `$2` user id, `$3` workspace slug.
    PAGES_LIVE_ACCESS_LINKS = "pages_live_access_links",
    "SELECT pp.project_id AS project_id, pm.role AS role, \
     p.owned_by_id AS owned_by_id, p.access AS access \
     FROM project_pages pp \
     INNER JOIN pages p ON pp.page_id = p.id \
     INNER JOIN projects pr ON pp.project_id = pr.id \
     INNER JOIN project_members pm ON pr.id = pm.project_id \
     INNER JOIN workspaces w ON pp.workspace_id = w.id \
     WHERE pp.deleted_at IS NULL AND pp.deleted_at IS NULL \
     AND p.deleted_at IS NULL AND pp.page_id = $1 AND pr.deleted_at IS NULL \
     AND pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
     AND w.slug = $3 \
     ORDER BY pp.created_at DESC",
    [
        ("project_id", Uuid),
        ("role", Int2),
        ("owned_by_id", Uuid),
        ("access", Int2),
    ]
}

plane_query! {
    /// The per-link guest check of `PageLiveAccessEndpoint.get`
    /// (`page/live.py:52-55`): `Project.objects.filter(pk=project_id,
    /// guest_view_all_features=True).exists()`.
    PAGES_PROJECT_GUEST_FEATURE = "pages_project_guest_feature",
    "SELECT 1 AS one FROM projects \
     WHERE deleted_at IS NULL AND guest_view_all_features AND id = $1 LIMIT 1",
    [("one", Int4)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PAGES_LIST,
    &PAGES_RETRIEVE,
    &PAGES_PROJECT_GUEST,
    &PAGES_SUMMARY_COUNTS,
    &PAGES_SUMMARY_COUNTS_GUEST,
    &PAGES_ISSUE_IDS,
    &PAGES_VERSIONS,
    &PAGES_VERSION_DETAIL,
    &PAGES_DESCRIPTION_BINARY,
    &PAGES_LIVE_ACCESS_LINKS,
    &PAGES_PROJECT_GUEST_FEATURE,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// [`PAGES_SELECT`] feeds the dynamic `?search=` / `order_by=` statement;
    /// [`PAGES_LIST`] is the catalogued default shape. If one changes without
    /// the other, `schema_check` would keep passing while asked requests went
    /// through an unverified statement.
    #[test]
    fn the_catalogued_list_is_the_dynamic_base_plus_the_default_order() {
        assert_eq!(
            format!("{PAGES_SELECT}{PAGES_GROUP_BY}{PAGES_DEFAULT_ORDER}"),
            PAGES_LIST.sql,
            "PAGES_SELECT + PAGES_GROUP_BY + PAGES_DEFAULT_ORDER must equal PAGES_LIST"
        );
        assert_eq!(
            format!("{PAGES_SELECT} AND p.id = $4{PAGES_GROUP_BY}{PAGES_DEFAULT_ORDER} LIMIT 1"),
            PAGES_RETRIEVE.sql,
            "PAGES_RETRIEVE must be the same statement with the pk filter"
        );
    }
}
