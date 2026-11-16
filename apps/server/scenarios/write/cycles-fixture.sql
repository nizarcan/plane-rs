-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Cycle-core write scenario fixture. Applied to both differential targets
-- before the baseline snapshot, so these rows are not step diffs until a
-- request changes them.
--
-- `CycleViewSet.destroy` hard-deletes `UserRecentVisit` rows for the cycle.
-- `UserRecentVisit` is a `SoftDeleteModel`, so the call is
-- `UserRecentVisit.objects.filter(...).delete(soft=False)`: the manager's
-- `deleted_at IS NULL` predicate is part of the SQL and a row that is already
-- soft-deleted survives. `Beta Release 2` (`cycle_beta_current`) is deleted at
-- the end of `cycles.yaml`; two live visits must go and the soft-deleted one
-- must stay.

INSERT INTO user_recent_visits
  (created_at, updated_at, id, entity_identifier, entity_name, visited_at,
   created_by_id, project_id, updated_by_id, user_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000021',
   'ea38d39b-18b7-4df1-be8b-f2ab43d4ab97', 'cycle',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000022',
   'ea38d39b-18b7-4df1-be8b-f2ab43d4ab97', 'cycle',
   '2026-01-01 00:00:00+00', '78100240-9cfd-4782-8627-25ab039c6e9a',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Already soft-deleted: must survive the manager-filtered hard delete.
  ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
   'f0000000-0000-4000-8000-000000000023',
   'ea38d39b-18b7-4df1-be8b-f2ab43d4ab97', 'cycle',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '2026-01-02 00:00:00+00');

-- `CycleFavoriteViewSet.destroy` calls `cycle_favorite.delete(soft=False)` on
-- an **instance** (`plane/app/views/cycle/base.py:587-595`), i.e. Django's
-- deletion collector (`plane/db/mixins.py:72-82`), not a single-row DELETE.
-- `user_favorites.parent` is a self-referential `on_delete=CASCADE` FK
-- (`plane/db/models/favorite.py:25-31`), so the delete must also remove every
-- row reachable through it — at every depth, including children whose own
-- `deleted_at` is set, because the collector reads the related model's plain
-- `_base_manager` (`django/db/models/deletion.py:398-410`). Only a fixture can
-- produce such a tree: no route creates a child favourite.
--
-- The tree hangs off the member's favourite of `Alpha Sprint 0`
-- (`cycle_alpha_archived`), which the scenario otherwise touches only with the
-- archived-PATCH 400 (cy-18) and never favourites. The archive POST of cy-43
-- targets `cycle_alpha_past` only, so this row survives it.
--
-- The live child belongs to the **admin**: the parent cascade ignores
-- `user_id`, so deleting the member's row must delete another user's child too.
-- Sequences are below the rich fixture's maximum (65535), so the H25 chain of
-- cy-30 is unchanged.

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The tree on `Alpha Sprint 0`: a live child (the admin's), a soft-deleted
  -- child, a live grandchild under the live child and a live grandchild under
  -- the **soft-deleted** child. Destroy must remove all five rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'e4000000-0000-4000-8000-000000000001', 'cycle',
   '2aa0956c-78ad-4002-8d90-1232ff2ae659', 'Alpha Sprint 0', FALSE,
   105.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'e4000000-0000-4000-8000-000000000002', 'cycle', NULL, 'Child Live', FALSE,
   106.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e4000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'e4000000-0000-4000-8000-000000000003', 'cycle', NULL, 'Child Soft Deleted',
   FALSE, 107.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e4000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'e4000000-0000-4000-8000-000000000004', 'cycle', NULL, 'Grandchild Live',
   FALSE, 108.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e4000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'e4000000-0000-4000-8000-000000000005', 'cycle', NULL,
   'Grandchild Of Soft Deleted', FALSE, 109.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e4000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
