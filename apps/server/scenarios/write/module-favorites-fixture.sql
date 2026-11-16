-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Module-favourites write scenario fixture (route 111, module set B). Applied to
-- both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are not step diffs until a request
-- deletes them.
--
-- Why it exists: `ModuleFavoriteViewSet.destroy` calls
-- `module_favorite.delete(soft=False)` on an **instance**
-- (`plane/app/views/module/base.py:833-844`), i.e. Django's deletion collector
-- (`plane/db/mixins.py:72-82`), not a single-row DELETE. `user_favorites.parent`
-- is a self-referential `on_delete=CASCADE` FK
-- (`plane/db/models/favorite.py:25-31`), so the delete must also remove every
-- row reachable through it — at every depth, including children whose own
-- `deleted_at` is set, because the collector reads the related model's plain
-- `_base_manager` (`django/db/models/deletion.py:398-410`). Only a fixture can
-- produce such a tree: no route creates a child favourite.
--
-- The tree hangs off the admin's favourite of `Alpha Paused`
-- (`module_paused`), a live module of project Alpha that no other step or
-- fixture favourites, so the whole run's H25 `MAX(sequence)` chain and the
-- module list's `is_favorite` annotation are deterministic on both targets.
-- `module_planned` stays free for wmod-49's create/duplicate pair.
--
-- The live child belongs to the **member**: the parent cascade ignores
-- `user_id`, so deleting the admin's row must delete another user's child too.
-- Sequences are above the rich fixture's maximum (155535) so the H25 chain that
-- starts at wmod-49 is the same on both targets.

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The tree on `Alpha Paused`: a live child (the member's), a soft-deleted
  -- child, a live grandchild under the live child and a live grandchild under
  -- the **soft-deleted** child. Destroy must remove all five rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'e1000000-0000-4000-8000-000000000001', 'module',
   '3dd8a2b9-d24e-4946-b348-e17e2e9676ac', 'Alpha Paused', FALSE,
   200000.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'e1000000-0000-4000-8000-000000000002', 'module', NULL, 'Child Live', FALSE,
   200001.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e1000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'e1000000-0000-4000-8000-000000000003', 'module', NULL, 'Child Soft Deleted',
   FALSE, 200002.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e1000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'e1000000-0000-4000-8000-000000000004', 'module', NULL, 'Grandchild Live',
   FALSE, 200003.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e1000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'e1000000-0000-4000-8000-000000000005', 'module', NULL,
   'Grandchild Of Soft Deleted', FALSE, 200004.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e1000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
