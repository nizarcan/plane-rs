-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Issue-export write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so this row is not
-- a step diff until a request changes it.
--
-- Why it exists: `plane_fix_tpl`'s four exporters all live in `fix-workspace`
-- and were initiated by `admin`. `ExportIssuesEndpoint.get`
-- (`plane/app/views/exporter/base.py:69-73`) filters `workspace__slug` and
-- `initiated_by=request.user`, so the history list needs a row in the *second*
-- workspace, initiated by that workspace's admin, to prove the join filters
-- rather than the four visible rows hiding it.
--
-- The token is a fixed 64-hex string: the history list renders it through the
-- `token` JSON-key rule (`scenarios/normalize.yaml:350-354`), which asserts the
-- `api_token` shape. Only a create generates a random 32-hex token, and the
-- scenario never lists the rows a create inserted (`plans/
-- ANALYTICS_EXPORTS.md` §4). `project` is an empty array so the row is a shape
-- the four template rows do not have.

INSERT INTO exporters
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   name, type, workspace_id, project, provider, status, reason, key, url,
   token, initiated_by_id, filters, rich_filters)
SELECT
  '2026-01-02 00:00:09+00', '2026-01-02 00:00:09+00', u.id, NULL,
  NULL, 'b9e00000-0000-4000-8000-000000000001',
  'Second workspace export', 'issue_exports', w.id, '{}',
  'json', 'completed', '', 'exports/second.json',
  'https://exports.example.test/second.json',
  'b9e0000000004000800000000000000100000000000000000000000000000000', u.id,
  NULL, '{}'
FROM workspaces w, users u
WHERE w.slug = 'fix-second' AND u.email = 'fix.nonmember@example.com';
