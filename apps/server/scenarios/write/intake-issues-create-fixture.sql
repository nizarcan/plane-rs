-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Intake-issue create scenario fixture (orders 55/59, plans/INTAKE.md set B).
-- Applied to both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are not step diffs.
--
-- Why it exists: the create view's triage fallback
-- (`plane/app/views/intake/base.py:247-257`) needs a project that has an intake
-- but **no** triage state, and the plain `plane_fix_tpl` fixture gives every
-- project with states a live Triage row (`tools/fixtures/build.py:753-822`).
--
-- * `epsilon` (EPS, `guest_view_all_features=True`, timezone Asia/Kolkata) gets
--   an intake and keeps its five default non-triage states, whose maximum
--   sequence is 55000 — so H19 (`plane/db/models/state.py:117-126`) creates the
--   fallback state with 55000+15000 = 70000, not the literal 65000.
-- * `gamma` (GAM, archived 40 days ago) is the same shape and is used to show
--   that a body which fails serializer validation still leaves the fallback
--   `states` row behind: a 400 that wrote a row. The archived project is
--   accepted by `Project.objects.get(pk=…)` and by `allow_permission` — neither
--   filters `archived_at`.
--
-- The triage rows are soft-deleted rather than removed so the partial unique
-- constraint `state_unique_name_project_when_deleted_at_null` allows the new
-- `Triage` row to be inserted on both targets.
--
-- `zeta` (ZET) is left untouched: it has no intake, no states and only the
-- admin as a member, and is the "no intake → 500 after the writes" case.

UPDATE states
   SET updated_at = '2026-01-02 00:00:00+00', deleted_at = '2026-01-02 00:00:00+00'
 WHERE project_id = '2e199eb6-4c83-4535-9362-e2beab71e9eb'
   AND "group" = 'triage'
   AND deleted_at IS NULL;

UPDATE states
   SET updated_at = '2026-01-02 00:00:00+00', deleted_at = '2026-01-02 00:00:00+00'
 WHERE project_id = '4f9d99bc-8083-4063-ac02-69dfb0e6ab95'
   AND "group" = 'triage'
   AND deleted_at IS NULL;

INSERT INTO intakes
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, name, description, is_default, view_props,
   logo_props)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'c9000000-0000-4000-8000-000000000001',
   '2e199eb6-4c83-4535-9362-e2beab71e9eb',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Epsilon Intake', '', FALSE, '{}', '{}'),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'c9000000-0000-4000-8000-000000000002',
   '4f9d99bc-8083-4063-ac02-69dfb0e6ab95',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Gamma Intake', '', FALSE, '{}', '{}');
