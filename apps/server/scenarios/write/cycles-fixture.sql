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
