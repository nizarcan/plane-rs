-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Analytics set C (route 203) fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until something changes them — and 203 only reads.
--
-- `plane_fix_tpl`'s own `issue_activities` rows are written by the real
-- `issue_activity` task at build time, so their `created_at` is "the hour the
-- template was built". 203 exports one **date** and renders each row's
-- `created_at`/`updated_at` verbatim inside the CSV, so the rows this scenario
-- turns on must have fixed, distinct timestamps: a shared timestamp would let
-- two runs order the CSV differently, and a build-hour timestamp could join a
-- fixed date on one run and not the next.
--
-- The date is 2026-01-05. Every row below is `fix-workspace` activity by
-- `fix.member@example.com` unless a comment says otherwise:
--
--   0001  field NULL, in alpha             included for UTC and for IST
--   0002  field 'comment'                  excluded by ~Q(field__in=[…])
--   0003  gamma, which is archived         included: 203 has no archived filter
--   0004  epsilon, where member is not a   excluded for member, included for
--         project member                   admin (who is an epsilon member)
--   0005  actor admin, in alpha            excluded when the target is member
--   0006  alpha, created 2026-01-04 20:00Z included only when the caller's
--         (= 2026-01-05 01:30 IST)         timezone puts it on 2026-01-05
--   0007  beta, soft-deleted               excluded by the manager predicate
--   0008  field 'vote'                     excluded by ~Q(field__in=[…])
--   0009  beta, plain update               included
--   0010  alpha, 2026-01-06                excluded by the date filter
--   0011  alpha, issue_id NULL             excluded: the issue joins are inner
--
-- 0006 also carries the formula- and CSV-quoting hazards: `old_value` starts
-- with `=`, and `new_value` has a comma, a quote and a newline in it.

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 01:00:00+00', '2026-01-05 01:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000001', 'issue.activity.created', NULL, NULL,
  NULL, '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 1;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 02:00:00+00', '2026-01-05 02:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000002', 'comment.activity.created',
  'comment', NULL, NULL, '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 2;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 03:00:00+00', '2026-01-05 03:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000003', 'issue.activity.updated',
  'priority', 'low', 'urgent', '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'GAM' AND i.project_id = p.id AND i.sequence_id = 1;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 04:00:00+00', '2026-01-05 04:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000004', 'issue.activity.updated',
  'state_id', 'Todo', 'In Progress', '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'EPS' AND i.project_id = p.id AND i.sequence_id = 1;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 05:00:00+00', '2026-01-05 05:00:00+00', a.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000005', 'issue.activity.updated',
  'priority', 'none', 'medium', '', '{}', i.id, p.id, w.id, a.id, NULL
FROM workspaces w, users a, projects p, issues i
WHERE w.slug = 'fix-workspace' AND a.email = 'fix.admin@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 3;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-04 20:00:00+00', '2026-01-04 20:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000006', 'issue.activity.updated',
  'priority', '=SUM(A1:A2)', E'a, "quoted"\nnewline', '', '{}', i.id, p.id,
  w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 4;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 06:00:00+00', '2026-01-05 06:00:00+00', u.id, NULL,
  '2026-01-05 07:00:00+00', 'b9a00000-0000-4000-8000-000000000007',
  'issue.activity.updated', 'target_date', 'old', 'new', '', '{}', i.id, p.id,
  w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'BET' AND i.project_id = p.id AND i.sequence_id = 1;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 08:00:00+00', '2026-01-05 08:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000008', 'issue_vote.activity.created',
  'vote', NULL, NULL, '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 5;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 07:00:00+00', '2026-01-05 07:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000009', 'issue.activity.updated',
  'priority', 'high', 'low', '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'BET' AND i.project_id = p.id AND i.sequence_id = 2;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-06 01:00:00+00', '2026-01-06 01:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000010', 'issue.activity.updated',
  'priority', 'low', 'high', '', '{}', i.id, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p, issues i
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP' AND i.project_id = p.id AND i.sequence_id = 6;

INSERT INTO issue_activities
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, verb,
   field, old_value, new_value, comment, attachments, issue_id, project_id,
   workspace_id, actor_id, epoch)
SELECT '2026-01-05 09:00:00+00', '2026-01-05 09:00:00+00', u.id, NULL, NULL,
  'b9a00000-0000-4000-8000-000000000011', 'issue.activity.updated',
  'priority', 'none', 'low', '', '{}', NULL, p.id, w.id, u.id, NULL
FROM workspaces w, users u, projects p
WHERE w.slug = 'fix-workspace' AND u.email = 'fix.member@example.com'
  AND p.identifier = 'ALP';
