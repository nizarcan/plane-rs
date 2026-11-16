-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Saved-view write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists:
--
--   * the destroy paths delete other users' `user_favorites` rows (H04 for
--     every user) and hard-delete every user's `user_recent_visits` row; the
--     rich fixture only has one admin favourite (`favorite_view`) and one
--     member recent visit with `project_id NULL`, which neither filter matches;
--   * the routes that create favourites belong to set B (183) and answer 501
--     on the Rust target until that lands;
--   * no route can create a `is_locked` view (read-only) or a view whose
--     `created_by`/`owned_by` differ from the caller, and both are needed for
--     the decorator-vs-handler 400/403 divergence;
--   * no route can leave `query` disagreeing with `filters`, which is what
--     proves the delete's H24 re-run.
--
-- Seeded views use sort orders below the fixture's maximum (85535 / 75535) so
-- the create chain is unchanged: the first created alpha view is 95535 and the
-- first workspace view 85535. Their `created_at` values are distinct because
-- `WorkspaceViewViewSet.get_queryset()` orders by `-created_at` with no
-- tiebreaker, so two equal timestamps would make the list order arbitrary.

INSERT INTO issue_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, name, description, query, filters,
   display_filters, display_properties, rich_filters, access, sort_order,
   logo_props, owned_by_id, is_locked, archived_at)
VALUES
  -- filters/query disagree: PATCH `{}` must recompute `query` from `filters`.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Stale Patch', '', '{}', '{"priority": "low"}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 100,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', FALSE, NULL),
  -- destroyed by vw-*; the soft-delete UPDATE must carry this recomputed query.
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Stale Delete', '', '{"wrong": true}',
   '{"state_group": "started", "type": "active"}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 101,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', FALSE, NULL),
  -- is_locked is read-only, so only seed data can produce the 400.
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000003',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Locked', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 102,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', TRUE, NULL),
  -- owned by the member, created by the admin: the admin passes the decorator
  -- (creator) and gets the handler's 400 "Only the owner…".
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000004',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Member Owns', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 103,
   '{}', '78100240-9cfd-4782-8627-25ab039c6e9a', FALSE, NULL),
  -- created by the member, owned by the admin: the member passes the DELETE
  -- decorator (creator) and gets the handler's 400.
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'a9000000-0000-4000-8000-000000000005',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Member Created', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 104,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', FALSE, NULL),
  -- workspace views: the workspace PATCH decorator has no admin fallback
  -- (roles=[] and level=WORKSPACE), so only the creator passes it.
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000006',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   NULL, 'Seed Ws Member Owns', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 105,
   '{}', '78100240-9cfd-4782-8627-25ab039c6e9a', FALSE, NULL),
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'a9000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   NULL, 'Seed Ws Member Created', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 106,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', FALSE, NULL),
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000008',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   NULL, 'Seed Ws Locked', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 107,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', TRUE, NULL);

-- Favourites the two destroy paths act on.
--
--   * `view_ws_public` (0e5e15ac-…, project NULL): two live favourites to be
--     H04-deleted by the workspace destroy, one row with a `project_id` that
--     the same destroy must leave alone (`project__isnull=True`).
--   * `view_alpha_started` (626adef8-…, project alpha): two live favourites to
--     be H04-deleted by the project destroy, one already-soft-deleted row and
--     one `project_id NULL` row that must both keep their `deleted_at`.
INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000101', 'view',
   '0e5e15ac-d652-4232-ac25-ac2dd1ead1c5', NULL, FALSE, 2000.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000102', 'view',
   '0e5e15ac-d652-4232-ac25-ac2dd1ead1c5', NULL, FALSE, 2001.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000103', 'view',
   '0e5e15ac-d652-4232-ac25-ac2dd1ead1c5', NULL, FALSE, 2002.0,
   '8e40753c-7792-49be-8620-598e938b1a71', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '8e40753c-7792-49be-8620-598e938b1a71',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000104', 'view',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', NULL, FALSE, 2003.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000105', 'view',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', NULL, FALSE, 2004.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000106', 'view',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', NULL, FALSE, 2005.0,
   'bc437914-c250-4c9f-ac69-85003fdf6205', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'bc437914-c250-4c9f-ac69-85003fdf6205',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000108', 'view',
   'a9000000-0000-4000-8000-000000000001', NULL, FALSE, 2007.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000107', 'view',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', NULL, FALSE, 2006.0,
   '8e40753c-7792-49be-8620-598e938b1a71', NULL, NULL, NULL,
   '8e40753c-7792-49be-8620-598e938b1a71',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

-- Recent visits: the project destroy hard-deletes live rows for every user and
-- leaves the soft-deleted and `project_id NULL` ones alone; the workspace
-- destroy does not touch this table at all.
INSERT INTO user_recent_visits
  (created_at, updated_at, id, entity_identifier, entity_name, visited_at,
   created_by_id, project_id, updated_by_id, user_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000201',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', 'view',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000202',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', 'view',
   '2026-01-01 00:00:00+00', '78100240-9cfd-4782-8627-25ab039c6e9a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000203',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', 'view',
   '2026-01-01 00:00:00+00', 'bc437914-c250-4c9f-ac69-85003fdf6205',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'bc437914-c250-4c9f-ac69-85003fdf6205',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000204',
   '626adef8-5e34-4a77-ab26-6bdeceb2afc0', 'view',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   NULL, NULL, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000205',
   '0e5e15ac-d652-4232-ac25-ac2dd1ead1c5', 'view',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   NULL, NULL, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
