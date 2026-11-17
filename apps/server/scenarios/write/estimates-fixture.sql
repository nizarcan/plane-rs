-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Estimates write-scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists (`plans/ESTIMATES.md` §2g.6, §4 "Fixture"):
--
--   * `plane_fix_tpl` seeds three live estimates with three distinct names —
--     alpha `Story Points`, alpha `Complexity`, beta `T-Shirt`
--     (`tools/fixtures/build.py:917-923`) — and no soft-deleted estimate at
--     any project. Estimate name uniqueness is a **partial** index,
--     `estimate_unique_name_project_when_deleted_at_null`
--     (`plane/db/models/estimate.py:30-36`), plus a `unique_together` that
--     includes `deleted_at` (`:29`): a create or a re-delete of a name only a
--     soft-deleted row holds must succeed, and without a seeded soft-deleted
--     twin that branch is unreachable.
--   * The live twin below holds the *same* `(name, project)` as the
--     soft-deleted row, so a `POST …/estimates/` with that name is a 200
--     (the partial index ignores the deleted row and the `unique_together`
--     sees a different `deleted_at`), and a `DELETE` of the live row is a 204
--     whose `deleted_at` write cannot violate the index either.
--   * The `fix-second`/theta row holds the same name in a different project:
--     uniqueness is per `(name, project)`, so a create of that name in alpha
--     must not conflict with it.
--
-- Ids are fixed and documented here; the scenarios' `vars` repeat them.
--
--   workspace          a91c3d80-5ed0-40f3-92a8-a28c015f4388  (fix-workspace)
--   workspace ws2      7045c0d6-5260-494d-a205-4cfbaab24fea  (fix-second)
--   project alpha      b0063033-54f9-49d8-b5f3-6b7a1551c6a6
--   project theta      08e2fc88-18a5-4758-87ec-5437bc0cf4bb  (ws2)
--   admin              d44ae570-3de4-4413-af08-f686bf5b7235
--   soft twin          b9100000-0000-4000-8000-000000000001  (alpha, deleted)
--   live twin          b9100000-0000-4000-8000-000000000002  (alpha)
--   other project      b9100000-0000-4000-8000-000000000003  (theta, ws2)
--
-- `created_at` is older than every rich-fixture row (base time
-- 2026-11-16T00:00:00Z) so these rows sort after the seeded ones wherever a
-- route orders by creation; each value is distinct because no ordering here
-- has a tiebreaker. `created_by_id` is the request-less fixture user (H01's
-- `updated_by` stays NULL) and `description` is the model's empty default.

-- ---------------------------------------------------------------------------
-- Estimates
-- ---------------------------------------------------------------------------
INSERT INTO estimates
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, name, description, type, last_used)
VALUES
  -- Soft-deleted "Legacy Points" in alpha: the name a live row may hold at
  -- the same time (partial unique index) and that a create may take again.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:04+00',
   'b9100000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Legacy Points', '', 'categories', FALSE),
  -- Live twin with the same (name, project): a DELETE of this row succeeds,
  -- and a create of "Legacy Points" in alpha is not a duplicate.
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9100000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Legacy Points', '', 'categories', FALSE),
  -- The same name in another project (and workspace): uniqueness is per
  -- (name, project), so alpha's rows are unaffected.
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9100000-0000-4000-8000-000000000003',
   '08e2fc88-18a5-4758-87ec-5437bc0cf4bb',
   '7045c0d6-5260-494d-a205-4cfbaab24fea',
   'Legacy Points', '', 'categories', FALSE);
