-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Saved-view-favourites write scenario fixture (route 184, views set B).
-- Applied to both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are not step diffs until a request
-- deletes them.
--
-- Why it exists: `IssueViewFavoriteViewSet.destroy` calls
-- `view_favorite.delete(soft=False)` on an **instance**
-- (`plane/app/views/view/base.py:471-479`), i.e. Django's deletion collector
-- (`plane/db/mixins.py:72-78`), not a single-row DELETE. `user_favorites.parent`
-- is a self-referential `on_delete=CASCADE` FK
-- (`plane/db/models/favorite.py:25-31`), so the delete must also remove every
-- row reachable through it — at every depth, including children whose own
-- `deleted_at` is set, because the collector reads the related model's plain
-- `_base_manager` (`django/db/models/deletion.py:398-410`). Only a fixture can
-- produce such a tree: no route creates a child favourite.
--
-- Every Alpha view of the rich fixture is favourited by some step of the
-- scenario (admin's `alpha_high`/`alpha_started`/`alpha_archived` creates and
-- member's `alpha_started`/`alpha_archived` creates), so the tree gets its own
-- live Alpha view: `Seed Favourite Cascade` (`a9000000-…-0020`), owned by the
-- admin and `access=1`, favourited by the admin and never touched by another
-- step. The live child belongs to the **member**: the parent cascade ignores
-- `user_id`, so deleting the admin's row must delete another user's child too.
--
-- The view is appended after the rich fixture's views (`sort_order` 900) so the
-- two project-view list responses this scenario checks grow by that one row on
-- both targets. Sequences are above the fixture's maximum so the H25 chain of
-- the create steps is the same on both targets.

INSERT INTO issue_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, name, description, query, filters,
   display_filters, display_properties, rich_filters, access, sort_order,
   logo_props, owned_by_id, is_locked, archived_at)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a9000000-0000-4000-8000-000000000020',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Favourite Cascade', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 1, 900,
   '{}', 'd44ae570-3de4-4413-af08-f686bf5b7235', FALSE, NULL);

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The tree on `Seed Favourite Cascade`: a live child (the member's), a
  -- soft-deleted child, a live grandchild under the live child and a live
  -- grandchild under the **soft-deleted** child. Destroy must remove all five
  -- rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'e2000000-0000-4000-8000-000000000001', 'view',
   'a9000000-0000-4000-8000-000000000020', 'Seed Favourite Cascade', FALSE,
   200000.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'e2000000-0000-4000-8000-000000000002', 'view', NULL, 'Child Live', FALSE,
   200001.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e2000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'e2000000-0000-4000-8000-000000000003', 'view', NULL, 'Child Soft Deleted',
   FALSE, 200002.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e2000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'e2000000-0000-4000-8000-000000000004', 'view', NULL, 'Grandchild Live',
   FALSE, 200003.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e2000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'e2000000-0000-4000-8000-000000000005', 'view', NULL,
   'Grandchild Of Soft Deleted', FALSE, 200004.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e2000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
