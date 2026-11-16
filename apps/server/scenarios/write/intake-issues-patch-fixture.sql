-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `IntakeIssueViewSet.partial_update` (orders 56/60) write scenario fixture.
-- Applied to both differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so none of these rows is a step diff until a
-- request changes it.
--
-- Why it exists — every branch of the view needs a link nothing else in
-- `plane_fix_tpl` provides:
--
--   * an accept needs an issue **in triage**, and the rich fixture's intake
--     links all point at Backlog/Todo/… issues;
--   * an accept with **no default state** needs a project with an intake, a
--     triage state and no `default` state — `alpha`/`beta` both have one, and
--     `zeta` (the no-intake project) has neither;
--   * the handler's guest branch needs an issue **created by the guest** with a
--     link whose `created_by` is (and is not) the guest: the rich fixture's
--     issue creators are only admin/member;
--   * the member branch needs "member is the issue's creator" and "member is
--     not", again distinct issues;
--   * nested-issue edits must start from a pristine issue (no assignees, no
--     labels, `priority: none`) so the replacement diff is unambiguous;
--   * `snoozed_till` needs a beta (Europe/Berlin) link with a stored
--     **non-midnight** value, so "unchanged stays non-midnight" is observable,
--     plus alpha (UTC) counterparts so the same body floors differently.
--
-- Ids are fixed and repeated in the scenario's `vars`:
--
--   ws                 a91c3d80-5ed0-40f3-92a8-a28c015f4388  (fix-workspace)
--   admin              d44ae570-3de4-4413-af08-f686bf5b7235
--   member             78100240-9cfd-4782-8627-25ab039c6e9a
--   guest              31a752d6-2f12-4ac9-b23f-ae260a96c148
--   project alpha      b0063033-54f9-49d8-b5f3-6b7a1551c6a6  (UTC)
--   project beta       ca8f652c-e89f-4c43-b91d-c2c6f9387509  (Europe/Berlin)
--   project iota       b1000000-0000-4000-8000-000000000001  (no default state)
--   alpha intake       826885ac-00cd-47cd-b54a-77a52004021c
--   beta intake        d32f147b-0b3c-4086-a617-2fe4ec3f4b27
--   iota intake        b1000000-0000-4000-8000-000000000002
--   alpha triage state 8e4eb107-d62e-4c38-a057-73a1d8317d8e
--   alpha backlog      5943efbb-5e08-4133-bf4a-e9547a8691ec
--   alpha todo         84b8f4a5-ed93-4af3-9817-e96436360ca3
--   iota triage state  b1000000-0000-4000-8000-000000000003
--   alpha label bug    51298d1d-71da-4345-baa7-3c68b16b1f00
--   alpha label feature 4e684b34-0406-4516-96ce-361f9f3f1665
--   member2            bc437914-c250-4c9f-ac69-85003fdf6205
--   alpha:105 (soft-deleted issue) 0c82df87-27e2-4961-9b88-c5446b1d5f02
--
-- A note on `sequence_id`: these rows are inserted directly, so no
-- `issue_sequences` row is written for them. Nothing on the PATCH path reads
-- that table (it is H11's create-time counter), and the values sit above the
-- fixture's own range so a create in another scenario cannot collide.

-- ---------------------------------------------------------------------------
-- Project iota: an intake, a triage state and no default state
-- ---------------------------------------------------------------------------
INSERT INTO projects
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, workspace_id, name, identifier, description, description_text,
   description_html, network, cycle_view, module_view, issue_views_view,
   page_view, intake_view, archive_in, close_in, logo_props, timezone,
   is_time_tracking_enabled, is_issue_type_enabled, guest_view_all_features,
   emoji, cover_image)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Iota No Default', 'IOT', 'no default state project', '{}', '{}',
   2, FALSE, TRUE, TRUE, TRUE, TRUE, 0, 0, '{}', 'UTC', FALSE, FALSE, FALSE,
   NULL, NULL);

INSERT INTO intakes
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, project_id, workspace_id, name, description, is_default, view_props,
   logo_props)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000002',
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Iota Intake', 'iota intake', TRUE, '{}', '{}');

INSERT INTO states
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, project_id, workspace_id, name, description, color, slug, sequence,
   "group", "default", is_triage, external_id, external_source)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000003',
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Iota Triage', '', '#4E5355', 'iota-triage', 65000, 'triage', FALSE, TRUE,
   NULL, NULL);

INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, project_id, workspace_id, state_id, name, description_json,
   description_html, description_stripped, priority, start_date, target_date,
   sequence_id, sort_order, completed_at, archived_at, is_draft, point,
   estimate_point_id, type_id, external_id, external_source)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000004',
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b1000000-0000-4000-8000-000000000003',
   'Iota triage issue', '{}', '<p>iota triage</p>', 'iota triage', 'none',
   NULL, NULL, 9001, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL);

INSERT INTO intake_issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, intake_id, issue_id, status, snoozed_till,
   duplicate_to_id, source, source_email, external_source, external_id, extra)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000005',
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b1000000-0000-4000-8000-000000000002',
   'b1000000-0000-4000-8000-000000000004',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}');

INSERT INTO project_members
  (created_at, updated_at, id, project_id, workspace_id, member_id, comment,
   role, view_props, default_props, preferences, sort_order, is_active,
   created_by_id, updated_by_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'b1000000-0000-4000-8000-000000000006',
   'b1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd44ae570-3de4-4413-af08-f686bf5b7235', '', 20,
   '{}', '{}', '{}', 65535, TRUE,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL);

-- ---------------------------------------------------------------------------
-- A workspace ADMIN whose alpha project role is GUEST (`tz_berlin`): the
-- permission's "project member + workspace admin" fallback admits them, the
-- handler's guest check is skipped (`not is_workspace_admin`), the intake
-- serializer runs — and the nested `issue_data` is *still* reduced, because
-- `base.py:405` tests the project role alone.
-- ---------------------------------------------------------------------------
UPDATE workspace_members SET role = 20
 WHERE workspace_id = 'a91c3d80-5ed0-40f3-92a8-a28c015f4388'
   AND member_id = '0e8e23a3-9c07-4333-82fd-4cd439b07279'
   AND deleted_at IS NULL;

UPDATE project_members SET role = 5
 WHERE project_id = 'b0063033-54f9-49d8-b5f3-6b7a1551c6a6'
   AND member_id = '0e8e23a3-9c07-4333-82fd-4cd439b07279'
   AND deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- Project kappa: an active membership but **no intake** (the PATCH's
-- `intake_id = NULL` path: the link lookup matches no row → 404). `zeta` of the
-- rich fixture has no `project_members` row at all, so its request would be the
-- permission's 403 before the view runs.
-- ---------------------------------------------------------------------------
INSERT INTO projects
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, workspace_id, name, identifier, description, description_text,
   description_html, network, cycle_view, module_view, issue_views_view,
   page_view, intake_view, archive_in, close_in, logo_props, timezone,
   is_time_tracking_enabled, is_issue_type_enabled, guest_view_all_features,
   emoji, cover_image)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b1000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Kappa No Intake', 'KAP', 'no intake project', '{}', '{}',
   2, FALSE, TRUE, TRUE, TRUE, TRUE, 0, 0, '{}', 'UTC', FALSE, FALSE, FALSE,
   NULL, NULL);

INSERT INTO project_members
  (created_at, updated_at, id, project_id, workspace_id, member_id, comment,
   role, view_props, default_props, preferences, sort_order, is_active,
   created_by_id, updated_by_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'b1000000-0000-4000-8000-000000000008',
   'b1000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd44ae570-3de4-4413-af08-f686bf5b7235', '', 20,
   '{}', '{}', '{}', 65535, TRUE,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL);

-- ---------------------------------------------------------------------------
-- Alpha (UTC): one issue per branch the view can take
-- ---------------------------------------------------------------------------
INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, project_id, workspace_id, state_id, name, description_json,
   description_html, description_stripped, priority, start_date, target_date,
   sequence_id, sort_order, completed_at, archived_at, is_draft, point,
   estimate_point_id, type_id, external_id, external_source)
VALUES
  -- ee…0001: triage, so `status: 1` moves it to the project default state.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '8e4eb107-d62e-4c38-a057-73a1d8317d8e',
   'Harness triage accept', '{}', '<p>triage accept</p>', 'triage accept',
   'high', NULL, NULL, 9001, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…0003: Backlog, so `status: 1` leaves the issue untouched.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness non-triage accept', '{}', '<p>non-triage accept</p>',
   'non-triage accept', 'low', NULL, NULL, 9003, 65535, NULL, NULL, FALSE,
   NULL, NULL, NULL, NULL, NULL),
  -- ee…0005: created by the member, so the member's creator branch passes.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'ee000000-0000-4000-8000-000000000005',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness member owned', '{}', '<p>member owned</p>', 'member owned',
   'none', NULL, NULL, 9005, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…0007: created by the admin, linked by the member: the member is not the
  -- creator, so the decorator answers 403.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000007',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness admin owned', '{}', '<p>admin owned</p>', 'admin owned', 'none',
   NULL, NULL, 9007, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  -- ee…0009: created by the guest, linked by the guest: the whole guest path
  -- runs (three-key projection, no intake serializer).
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL,
   'ee000000-0000-4000-8000-000000000009',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness guest owned', '{}', '<p>guest owned</p>', 'guest owned', 'none',
   NULL, NULL, 9009, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  -- ee…000b: created by the guest, linked by the admin: the guest passes the
  -- decorator's creator branch and the handler answers 400.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL,
   'ee000000-0000-4000-8000-00000000000b',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness guest not creator', '{}', '<p>guest not creator</p>',
   'guest not creator', 'none', NULL, NULL, 9011, 65535, NULL, NULL, FALSE,
   NULL, NULL, NULL, NULL, NULL),
  -- ee…001c: a triage issue whose link is used for the combined
  -- `status: 1` + nested-edit step: the nested save runs first, then the accept
  -- move writes the *stale cached* issue object, so the rename is reverted.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000001c',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '8e4eb107-d62e-4c38-a057-73a1d8317d8e',
   'Harness triage stale', '{}', '<p>triage stale</p>', 'triage stale',
   'medium', NULL, NULL, 9035, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…001e: owned by someone else, linked by someone else: the workspace
  -- admin with a guest project role is admitted by the permission's fallback.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000001e',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness workspace admin guest', '{}', '<p>ws admin guest</p>',
   'ws admin guest', 'none', NULL, NULL, 9037, 65535, NULL, NULL, FALSE,
   NULL, NULL, NULL, NULL, NULL),
  -- ee…0011: pristine, for the writable `created_by` step.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000011',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness created by', '{}', '<p>created by</p>', 'created by', 'none',
   NULL, NULL, 9013, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  -- ee…0013: pristine, for `duplicate_to`.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000013',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness duplicate target', '{}', '<p>duplicate target</p>',
   'duplicate target', 'none', NULL, NULL, 9015, 65535, NULL, NULL, FALSE,
   NULL, NULL, NULL, NULL, NULL),
  -- ee…0015: no assignees, no labels, `priority: none` — the nested-issue step's
  -- starting point.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000015',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness nested issue', '{}', '<p>nested issue</p>', 'nested issue',
   'none', NULL, NULL, 9017, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…0017: for `skip_activity`.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000017',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness skip activity', '{}', '<p>skip activity</p>', 'skip activity',
   'none', NULL, NULL, 9019, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…0019: linked by a soft-deleted link row (the link lookup hides it).
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000019',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness soft link issue', '{}', '<p>soft link</p>', 'soft link', 'none',
   NULL, NULL, 9021, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  -- ee…001a: soft-deleted, linked by a live link: `issue` in the body 404s,
  -- while a body without `issue` still renders (the re-fetch joins the base
  -- manager).
  ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-02 00:00:00+00',
   'ee000000-0000-4000-8000-00000000001a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness deleted issue', '{}', '<p>deleted issue</p>', 'deleted issue',
   'none', NULL, NULL, 9023, 65535, NULL, NULL, FALSE, NULL, NULL, NULL,
   NULL, NULL),
  -- ee…0029: alpha counterpart of the snooze datetime step (UTC project).
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000029',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '5943efbb-5e08-4133-bf4a-e9547a8691ec',
   'Harness snooze utc', '{}', '<p>snooze utc</p>', 'snooze utc', 'none',
   NULL, NULL, 9025, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL);

INSERT INTO intake_issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, intake_id, issue_id, status, snoozed_till,
   duplicate_to_id, source, source_email, external_source, external_id, extra)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000001',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000004',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000003',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'ee000000-0000-4000-8000-000000000006',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000005',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'ee000000-0000-4000-8000-000000000008',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000007',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL,
   'ee000000-0000-4000-8000-00000000000a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000009',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000000c',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-00000000000b',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000001f',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-00000000001e',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000001d',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-00000000001c',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000010',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000011',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000012',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000013',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000014',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000015',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000016',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000017',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  -- A soft-deleted link: `IntakeIssue.objects.get(...)` is a 404.
  ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, '2026-01-02 00:00:00+00',
   'ee000000-0000-4000-8000-000000000018',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000019',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-00000000001b',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-00000000001a',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000028',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'ee000000-0000-4000-8000-000000000029',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}');

-- The nested-issue step replaces the through rows of issue ee…0015: one live
-- assignee (member2) and one live label (alpha bug), so the soft delete of the
-- old rows and the insert of the new ones both show up in the diff.
INSERT INTO issue_assignees
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, issue_id, assignee_id)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000030',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'ee000000-0000-4000-8000-000000000015',
   'bc437914-c250-4c9f-ac69-85003fdf6205');

INSERT INTO issue_labels
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, issue_id, label_id)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000031',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'ee000000-0000-4000-8000-000000000015',
   '51298d1d-71da-4345-baa7-3c68b16b1f00');

-- ---------------------------------------------------------------------------
-- Beta (Europe/Berlin): the snooze conversions
-- ---------------------------------------------------------------------------
-- Beta (Europe/Berlin): the snooze conversions
-- ---------------------------------------------------------------------------
INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at,
   id, project_id, workspace_id, state_id, name, description_json,
   description_html, description_stripped, priority, start_date, target_date,
   sequence_id, sort_order, completed_at, archived_at, is_draft, point,
   estimate_point_id, type_id, external_id, external_source)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000021',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '85defd20-a58b-40d3-953f-0bba411719e1',
   'Harness snooze held', '{}', '<p>snooze held</p>', 'snooze held', 'none',
   NULL, NULL, 9027, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000023',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '85defd20-a58b-40d3-953f-0bba411719e1',
   'Harness snooze datetime', '{}', '<p>snooze datetime</p>',
   'snooze datetime', 'none', NULL, NULL, 9029, 65535, NULL, NULL, FALSE,
   NULL, NULL, NULL, NULL, NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000025',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '85defd20-a58b-40d3-953f-0bba411719e1',
   'Harness snooze date', '{}', '<p>snooze date</p>', 'snooze date', 'none',
   NULL, NULL, 9031, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000027',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '85defd20-a58b-40d3-953f-0bba411719e1',
   'Harness snooze null', '{}', '<p>snooze null</p>', 'snooze null', 'none',
   NULL, NULL, 9033, 65535, NULL, NULL, FALSE, NULL, NULL, NULL, NULL, NULL);

INSERT INTO intake_issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, intake_id, issue_id, status, snoozed_till,
   duplicate_to_id, source, source_email, external_source, external_id, extra)
VALUES
  -- status 0 with a stored non-midnight snooze: a PATCH that does not touch
  -- `snoozed_till` must leave it exactly as it is (H16's `has_changed` gate).
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000020',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd32f147b-0b3c-4086-a617-2fe4ec3f4b27',
   'ee000000-0000-4000-8000-000000000021',
   0, '2026-03-04 08:00:00+00', NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000022',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd32f147b-0b3c-4086-a617-2fe4ec3f4b27',
   'ee000000-0000-4000-8000-000000000023',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000024',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd32f147b-0b3c-4086-a617-2fe4ec3f4b27',
   'ee000000-0000-4000-8000-000000000025',
   -2, NULL, NULL, 'IN_APP', NULL, NULL, NULL, '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'ee000000-0000-4000-8000-000000000026',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd32f147b-0b3c-4086-a617-2fe4ec3f4b27',
   'ee000000-0000-4000-8000-000000000027',
   0, '2026-05-01 00:00:00+00', NULL, 'IN_APP', NULL, NULL, NULL, '{}');
