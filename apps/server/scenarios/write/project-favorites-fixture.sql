-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Project-favourites write scenario fixture (route 150, project satellite
-- routes). Applied to both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are not step diffs until a
-- request deletes them.
--
-- Why it exists: `ProjectFavoritesViewSet.destroy` calls
-- `project_favorite.delete(soft=False)` on an **instance**
-- (`plane/app/views/project/base.py:527-535`), i.e. Django's deletion collector
-- (`plane/db/mixins.py:72-82`), not a single-row DELETE. `user_favorites.parent`
-- is a self-referential `on_delete=CASCADE` FK
-- (`plane/db/models/favorite.py:25-31`), so the delete must also remove every
-- row reachable through it — at every depth, including children whose own
-- `deleted_at` is set, because the collector reads the related model's plain
-- `_base_manager` (`django/db/models/deletion.py:398-410`). Only a fixture can
-- produce such a tree: no route creates a child favourite.
--
-- The tree hangs off the **admin's** favourite of project Beta, which no other
-- step or fixture row creates (the rich fixture's Beta favourite belongs to the
-- member). Alpha is left alone: px-10/px-15 create and destroy the member's
-- Alpha favourite, and px-27 archives Alpha.
--
-- The live child belongs to the **member**: the parent cascade ignores
-- `user_id`, so deleting the admin's row must delete another user's child too.
-- Sequences are below the rich fixture's maximum (65535) so the H25 chain that
-- starts at px-10 is the same on both targets.

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The tree on Beta: a live child (the member's), a soft-deleted child, a
  -- live grandchild under the live child and a live grandchild under the
  -- **soft-deleted** child. Destroy must remove all five rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'e3000000-0000-4000-8000-000000000001', 'project',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', 'Beta Mobile', FALSE,
   100.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'e3000000-0000-4000-8000-000000000002', 'project', NULL, 'Child Live', FALSE,
   101.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e3000000-0000-4000-8000-000000000001',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'e3000000-0000-4000-8000-000000000003', 'project', NULL, 'Child Soft Deleted',
   FALSE, 102.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e3000000-0000-4000-8000-000000000001',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'e3000000-0000-4000-8000-000000000004', 'project', NULL, 'Grandchild Live',
   FALSE, 103.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e3000000-0000-4000-8000-000000000002',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'e3000000-0000-4000-8000-000000000005', 'project', NULL,
   'Grandchild Of Soft Deleted', FALSE, 104.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'e3000000-0000-4000-8000-000000000003',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
