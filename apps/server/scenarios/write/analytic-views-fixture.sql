-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Stored-view write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists:
--
--   * `plane_fix_tpl` has two live analytic views in `fix-workspace` and none
--     anywhere else, so nothing exercises the default manager's
--     `deleted_at IS NULL` predicate (a soft-deleted pk must be a 404 on PATCH
--     and DELETE and absent from the list) or the `workspaces` join
--     (`AnalyticViewViewset.get_queryset`, `plane/app/views/analytic/base.py:
--     192-193`) that makes a live row of another workspace a 404 through
--     `fix-workspace`;
--   * no route can produce a row whose `query` disagrees with its
--     `query_dict`: a PATCH always stores `query = {}` (`serializers/analytic.py
--     :25-31`), and only a seeded row can prove that the reset happens.
--
-- Timestamps are fixed and distinct: the list orders by `-created_at` with no
-- tiebreaker (`Meta.ordering`, `plane/db/models/analytic.py:22`). They sort
-- after the template's own rows (built at fixture time), so the created rows of
-- a step always sort first.

INSERT INTO analytic_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description, query, query_dict)
SELECT
  '2026-01-02 00:00:01+00', '2026-01-02 00:00:01+00', u.id, NULL,
  '2026-01-03 00:00:01+00', 'b9f00000-0000-4000-8000-000000000001',
  w.id, 'Seed Deleted View', '', '{"priority__in": "urgent"}',
  '{"priority": "urgent"}'
FROM workspaces w, users u
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.admin@example.com';

-- The same soft-deleted shape in the second workspace, reached only through
-- `fix-second`.
INSERT INTO analytic_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description, query, query_dict)
SELECT
  '2026-01-02 00:00:02+00', '2026-01-02 00:00:02+00', u.id, NULL,
  '2026-01-03 00:00:02+00', 'b9f00000-0000-4000-8000-000000000002',
  w.id, 'Seed Second Deleted', '', '{}', '{}'
FROM workspaces w, users u
WHERE w.slug = 'fix-second' AND u.email = 'fix.nonmember@example.com';

-- Live, but in `fix-second`: a 404 through `/workspaces/fix-workspace/…`, a
-- 200/204 through `/workspaces/fix-second/…` for that workspace's admin.
INSERT INTO analytic_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description, query, query_dict)
SELECT
  '2026-01-02 00:00:03+00', '2026-01-02 00:00:03+00', u.id, NULL,
  NULL, 'b9f00000-0000-4000-8000-000000000003',
  w.id, 'Seed Second Live', 'seeded in the other workspace',
  '{"state__group__in": "started"}', '{"state_group": "started"}'
FROM workspaces w, users u
WHERE w.slug = 'fix-second' AND u.email = 'fix.nonmember@example.com';

-- Live in `fix-workspace`, with a `query` that contradicts its `query_dict`:
-- the PATCH `{}` must replace it with `{}` and leave `query_dict` alone.
INSERT INTO analytic_views
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description, query, query_dict)
SELECT
  '2026-01-02 00:00:04+00', '2026-01-02 00:00:04+00', u.id, NULL,
  NULL, 'b9f00000-0000-4000-8000-000000000004',
  w.id, 'Seed Stale Query', 'query disagrees with query_dict',
  '{"wrong": true}', '{"priority": "low"}'
FROM workspaces w, users u
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.admin@example.com';
