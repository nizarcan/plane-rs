-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Page-favourites write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists:
--
--   * the rich fixture links only two pages to project Alpha (`page_project`,
--     public, and `page_child`, private) and neither is archived; the create
--     gate has no page `archived_at` predicate, but proving that needs an
--     archived page with a live Alpha link;
--   * a soft-deleted `project_pages` link must make the page invisible to the
--     gate (the query joins the through table with `deleted_at IS NULL`);
--   * `PageFavoriteViewSet.destroy` is `page_favorite.delete(soft=False)`,
--     which runs Django's deletion collector: the favourite **and every row
--     reachable through `user_favorites.parent`** (`on_delete=CASCADE`,
--     `plane/db/models/favorite.py:25-31`) are hard-deleted, at every depth,
--     including children whose own `deleted_at` is set (the collector reads the
--     related model's plain `_base_manager`). Only a fixture can produce that
--     tree, since no route creates a child favourite;
--   * a soft-deleted favourite of the caller must stay a 404 for the destroy
--     lookup (`UserFavorite.objects` filters `deleted_at IS NULL`).
--
-- All sequences are above the rich fixture's maximum (155535), so the H25
-- `MAX(sequence) + 10000` chain of the create steps is deterministic on both
-- targets. `d0…` ids are pages, `d1…` through rows, `d2…` favourites.

INSERT INTO pages
  (created_at, updated_at, id, name, description_json, description_html,
   access, created_by_id, owned_by_id, updated_by_id, workspace_id, color,
   archived_at, is_locked, parent_id, view_props, logo_props,
   description_binary, is_global, deleted_at, moved_to_page, moved_to_project,
   external_id, external_source, sort_order, description_stripped)
VALUES
  -- Archived, but linked to Alpha: the create gate never looks at the page's
  -- own `archived_at`.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd0000000-0000-4000-8000-000000000001', 'Favourite Archived Page', '{}',
   '<p>archived</p>', 0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '#F59E0B', '2026-01-15', FALSE,
   NULL, '{"full_width": false}', '{}', NULL, FALSE, NULL, NULL, NULL, NULL,
   NULL, 1000000, NULL),
  -- The parent of the favourite tree the destroy collector must delete.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd0000000-0000-4000-8000-000000000002', 'Favourite Cascade Page', '{}',
   '<p>cascade</p>', 0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '#F59E0B', NULL, FALSE, NULL,
   '{"full_width": false}', '{}', NULL, FALSE, NULL, NULL, NULL, NULL, NULL,
   1000001, NULL),
  -- Linked to Alpha through a **soft-deleted** `project_pages` row: 404.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd0000000-0000-4000-8000-000000000003', 'Favourite Detached Page', '{}',
   '<p>detached</p>', 0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '#F59E0B', NULL, FALSE, NULL,
   '{"full_width": false}', '{}', NULL, FALSE, NULL, NULL, NULL, NULL, NULL,
   1000002, NULL),
  -- Carries a soft-deleted favourite of the caller: the destroy lookup misses.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd0000000-0000-4000-8000-000000000004', 'Favourite Soft Deleted Row Page',
   '{}', '<p>soft deleted favourite</p>', 0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '#F59E0B', NULL, FALSE, NULL,
   '{"full_width": false}', '{}', NULL, FALSE, NULL, NULL, NULL, NULL, NULL,
   1000003, NULL);

INSERT INTO project_pages
  (created_at, updated_at, id, created_by_id, page_id, project_id,
   updated_by_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd1000000-0000-4000-8000-000000000001',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd0000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd1000000-0000-4000-8000-000000000002',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd0000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
   'd1000000-0000-4000-8000-000000000003',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd0000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd1000000-0000-4000-8000-000000000004',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd0000000-0000-4000-8000-000000000004',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The tree on `Favourite Cascade Page`: a live child, a soft-deleted child,
  -- a live grandchild under the live child and a live grandchild under the
  -- **soft-deleted** child. The destroy must remove all five rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd2000000-0000-4000-8000-000000000001', 'page',
   'd0000000-0000-4000-8000-000000000002', 'Favourite Cascade Page', FALSE,
   200000.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd2000000-0000-4000-8000-000000000002', 'page', NULL, 'Child Live', FALSE,
   200001.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'd2000000-0000-4000-8000-000000000003', 'page', NULL, 'Child Soft Deleted',
   FALSE, 200002.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd2000000-0000-4000-8000-000000000004', 'page', NULL, 'Grandchild Live',
   FALSE, 200003.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'd2000000-0000-4000-8000-000000000005', 'page', NULL,
   'Grandchild Of Soft Deleted', FALSE, 200004.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Soft-deleted favourite of the caller: the destroy lookup must miss it.
  ('2026-01-01 00:00:06+00', '2026-01-02 00:00:00+00',
   'd2000000-0000-4000-8000-000000000010', 'page',
   'd0000000-0000-4000-8000-000000000004', 'Soft Deleted Favourite', FALSE,
   200010.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00');
