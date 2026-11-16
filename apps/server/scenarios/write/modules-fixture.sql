-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Module-core write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists: the archive POST and the destroy both delete `UserFavorite`
-- rows, and destroy hard-deletes `UserRecentVisit` rows. The rich fixture's own
-- module favourite has `project_id NULL`, which neither filter matches, and the
-- routes that create favourites are another set's route (110) and answer 501 on
-- the Rust target until that set lands. Seeding the rows here keeps this
-- scenario independent of set B.
--
--   * `module_alpha_completed` — one favourite for the admin and one for the
--     member: the archive POST's H04 removes **both**.
--   * `module_alpha_planned` — one favourite for the admin and one for the
--     member, plus a recent visit for each: destroy removes only the caller's
--     (admin's) favourite and hard-deletes **every** recent visit.

INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000001', 'module',
   'e03eff9f-ec87-4133-bee9-474d29c4d397', NULL, FALSE, 1000.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000002', 'module',
   'e03eff9f-ec87-4133-bee9-474d29c4d397', NULL, FALSE, 1001.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000003', 'module',
   'cac530e3-b832-4ca9-aef9-f4427b1a8f86', NULL, FALSE, 1002.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000004', 'module',
   'cac530e3-b832-4ca9-aef9-f4427b1a8f86', NULL, FALSE, 1003.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

INSERT INTO user_recent_visits
  (created_at, updated_at, id, entity_identifier, entity_name, visited_at,
   created_by_id, project_id, updated_by_id, user_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000011',
   'cac530e3-b832-4ca9-aef9-f4427b1a8f86', 'module',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'f0000000-0000-4000-8000-000000000012',
   'cac530e3-b832-4ca9-aef9-f4427b1a8f86', 'module',
   '2026-01-01 00:00:00+00', '78100240-9cfd-4782-8627-25ab039c6e9a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);
