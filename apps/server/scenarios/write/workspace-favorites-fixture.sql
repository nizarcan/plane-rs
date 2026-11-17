-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Workspace favourites write scenario fixture (routes 212 and 213). Applied to
-- both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are not step diffs until a request
-- changes them.
--
-- Why it exists:
--
--   * `WorkspaceFavoriteEndpoint.delete` is `favorite.delete(soft=False)`
--     (`plane/app/views/workspace/favorite.py:92`), i.e. Django's deletion
--     collector over the self-referential `user_favorites.parent` CASCADE
--     (`plane/db/models/favorite.py:25-31`). It removes every descendant at
--     every depth, soft-deleted children included, and crosses users — only a
--     fixture can build such a tree, because no route creates a child row
--     without the parent being the caller's own live row.
--   * `parent` is an unvalidated writable FK
--     (`SERIALIZERS.md` 1982-1984): a favourite can be parented under another
--     user's row (the tree's live child) and under a row in **another
--     workspace** (`d2…0020`, `fix-second`). The scenario PATCHes and POSTs
--     both.
--   * PATCH's `parent` queryset is `UserFavorite.objects`, so a **soft-deleted**
--     row must be `Invalid pk "…" - object does not exist.` — the
--     `d2000000-…-0010` control.
--   * `fix-nologo` has no favourites, so its `MAX(sequence)` is NULL and the
--     body's `sequence` survives H25 (`plane/db/models/favorite.py:62-63`).
--   * `fix-deleted` is a soft-deleted workspace: POST's
--     `Workspace.objects.get` 404s while PATCH/DELETE's
--     `workspace__slug` join has no `deleted_at` predicate.
--   * the partial unique index `(entity_type, entity_identifier, user) WHERE
--     deleted_at IS NULL` (`plane/db/models/favorite.py:35-41`) has no
--     workspace column, so a favourite that lives in another workspace makes a
--     repeat POST a duplicate 400 instead of the early-return 200
--     (`d3…0040` in `fix-second`).
--
-- Sequences: the tree is the rich fixture's new maximum (300000-300004), so the
-- first `fix-workspace` create's H25 value is 310004 on both targets; the
-- `fix-second` rows sit at 500000-500002, the `fix-deleted` row at 600000 and
-- `fix-nologo` deliberately has none. `d1…` ids are views, `d2…` favourites,
-- `d3…` entity identifiers.

INSERT INTO issue_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, name, description, query, filters,
   display_filters, display_properties, rich_filters, access, sort_order,
   logo_props, owned_by_id, is_locked, archived_at)
VALUES
  -- Alpha, private, owned by the member: the admin's POST must 404 (the gate's
  -- `owned_by | access=1` branch).
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'd1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'Seed Member Private View', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 0, 950,
   '{}', '78100240-9cfd-4782-8627-25ab039c6e9a', FALSE, NULL),
  -- A workspace view with the same shape: the workspace-view gate's
  -- `owned_by | access=1` branch must also 404 for the admin.
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'd1000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   NULL,
   'Seed Member Private Workspace View', '', '{}', '{}',
   '{"layout": "list"}', '{"assignee": true}', '{}', 0, 951,
   '{}', '78100240-9cfd-4782-8627-25ab039c6e9a', FALSE, NULL);

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  -- The cascade tree on the admin's folder: a live child owned by the
  -- **member**, a soft-deleted child, a live grandchild under the live child
  -- and a live grandchild under the **soft-deleted** child. The scenario's
  -- DELETE must remove all five rows.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd2000000-0000-4000-8000-000000000001', 'folder', NULL, 'Seed Cascade Folder',
   TRUE, 300000.0, 'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd2000000-0000-4000-8000-000000000002', 'folder', NULL, 'Seed Member Child',
   FALSE, 300001.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000001', NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'd2000000-0000-4000-8000-000000000003', 'folder', NULL,
   'Seed Soft Deleted Child', FALSE, 300002.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000001', NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd2000000-0000-4000-8000-000000000004', 'folder', NULL, 'Seed Live Grandchild',
   FALSE, 300003.0, 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000002', NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'd2000000-0000-4000-8000-000000000005', 'folder', NULL,
   'Seed Grandchild Of Soft Deleted', FALSE, 300004.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'd2000000-0000-4000-8000-000000000003', NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- The soft-deleted parent control: `UserFavorite.objects` filters it, so both
  -- POST and PATCH must answer DRF's `Invalid pk … object does not exist.`.
  ('2026-01-01 00:00:06+00', '2026-01-02 00:00:00+00',
   'd2000000-0000-4000-8000-000000000010', 'folder', NULL,
   'Seed Soft Deleted Parent', TRUE, 300010.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '2026-01-02 00:00:00+00'),
  -- `fix-second` (a different workspace): the cross-workspace parent target and
  -- a live row another user's child can point at.
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   'd2000000-0000-4000-8000-000000000020', 'folder', NULL,
   'Seed Second Workspace Folder', TRUE, 500000.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', NULL),
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'd2000000-0000-4000-8000-000000000021', 'folder', NULL,
   'Seed Second Workspace Row', FALSE, 500001.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', NULL),
  -- The seeded duplicate: a live favourite in `fix-second` with an
  -- `entity_identifier` a `fix-workspace` POST reuses with `project_id` set to
  -- Theta. The early-return lookup filters on the **URL** workspace, misses,
  -- and the INSERT hits the unique index.
  ('2026-01-01 00:00:09+00', '2026-01-01 00:00:09+00',
   'd2000000-0000-4000-8000-000000000040', 'folder',
   'd3000000-0000-4000-8000-000000000040', 'Seed Foreign Duplicate', FALSE,
   500002.0, '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', NULL),
  -- `fix-deleted` is soft-deleted: POST 404s at `Workspace.objects.get`, while
  -- PATCH/DELETE reach this row through the `workspace__slug` join.
  ('2026-01-01 00:00:10+00', '2026-01-01 00:00:10+00',
   'd2000000-0000-4000-8000-000000000030', 'folder', NULL,
   'Seed Deleted Workspace Row', FALSE, 600000.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL, NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   '4a495179-13e7-4373-921c-d9681a53ffd3', NULL);
