-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Intake write scenario fixture (plans/INTAKE.md sets A and D). Applied to both
-- differential targets before the baseline snapshot
-- (`vars: { fixture_sql: … }`), so these rows are never step diffs until a
-- request changes them.
--
-- Why it exists:
--
--   * `plane_fix_tpl` gives alpha and beta one intake each and **both are
--     `is_default=True`**, so the DELETE-success path (which the default intake
--     answers with a 400) has no row to work on;
--   * the same fixture has no soft-deleted intake, which is the DELETE 500
--     (`None.is_default`) branch;
--   * every issue in the fixture is created by the admin, so neither the
--     decorator's creator branch (a MEMBER deleting an issue they created) nor
--     a GUEST's creator branch can be reached without new rows;
--   * no live `IntakeIssue` points at a soft-deleted issue, which is the
--     DELETE 500 (`None.delete()`) after the link lookup succeeds;
--   * no issue exists in Zeta Empty (no intake at all), which is the
--     `intake_id=NULL` → 404/DoesNotExist link lookup;
--   * a MEMBER-created issue in **beta** is needed to show that the decorator's
--     creator lookup is unscoped by the URL project (alpha's MEMBER passes it,
--     then the handler's alpha link lookup 404s; a scoped check would 403).
--
-- Ids are fixed and documented here; the scenario's `vars` repeat them.
--
--   workspace fix-workspace  a91c3d80-5ed0-40f3-92a8-a28c015f4388
--   project alpha            b0063033-54f9-49d8-b5f3-6b7a1551c6a6
--   project beta             ca8f652c-e89f-4c43-b91d-c2c6f9387509
--   project zeta             95100651-3264-4bd8-b2f7-5d6a082c9851
--   intake alpha (default)   826885ac-00cd-47cd-b54a-77a52004021c
--   admin                    d44ae570-3de4-4413-af08-f686bf5b7235
--   member                   78100240-9cfd-4782-8627-25ab039c6e9a
--   guest                    31a752d6-2f12-4ac9-b23f-ae260a96c148
--   nonmember                e15c61cc-f495-4e69-a88f-9c2cf5cdca7b

-- ---------------------------------------------------------------------------
-- Intakes: two non-default rows to delete through each alias, one to delete as
-- a MEMBER, and one already soft-deleted (the `None.is_default` 500).
-- ---------------------------------------------------------------------------
INSERT INTO intakes
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, name, description, is_default, view_props,
   logo_props)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Harness Intake One',
   'deleted through /intakes/', FALSE, '{}', '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Harness Intake Two',
   'deleted through /inboxes/', FALSE, '{}', '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Harness Intake Three',
   'deleted by a project member', FALSE, '{}', '{}'),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-02-01 00:00:00+00',
   'cc000000-0000-4000-8000-000000000004',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Harness Intake Deleted',
   'soft-deleted; DELETE is the `None.is_default` 500', FALSE, '{}', '{}');

-- ---------------------------------------------------------------------------
-- Issues, cloned from an alpha row so every column has a live value.
--
--   010  alpha, created by the MEMBER   (creator branch: MEMBER deletes)
--   011  alpha, created by the GUEST    (creator branch: GUEST deletes)
--   012  alpha, soft-deleted            (DELETE → `None.delete()` → 500)
--   013  beta, created by the MEMBER    (creator lookup is unscoped)
--   014  zeta, created by the ADMIN     (project with no intake → 404)
-- ---------------------------------------------------------------------------
INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, parent_id, state_id, point, estimate_point_id,
   name, description_json, description_html, description_stripped,
   description_binary, priority, start_date, target_date, sequence_id,
   sort_order, completed_at, archived_at, is_draft, external_source,
   external_id, type_id)
SELECT '2026-03-01 00:00:00+00', '2026-03-01 00:00:00+00',
       '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
       'cc000000-0000-4000-8000-000000000010',
       project_id, workspace_id, NULL, state_id, NULL, NULL,
       'Harness member issue', description_json, '<p>Harness member issue</p>',
       'Harness member issue', NULL, priority, NULL, NULL, sequence_id + 1000,
       sort_order, NULL, NULL, FALSE, NULL, NULL, NULL
FROM issues WHERE id = '27a64497-44aa-446b-8136-8197010dea71';

INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, parent_id, state_id, point, estimate_point_id,
   name, description_json, description_html, description_stripped,
   description_binary, priority, start_date, target_date, sequence_id,
   sort_order, completed_at, archived_at, is_draft, external_source,
   external_id, type_id)
SELECT '2026-03-01 00:00:01+00', '2026-03-01 00:00:01+00',
       '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL,
       'cc000000-0000-4000-8000-000000000011',
       project_id, workspace_id, NULL, state_id, NULL, NULL,
       'Harness guest issue', description_json, '<p>Harness guest issue</p>',
       'Harness guest issue', NULL, priority, NULL, NULL, sequence_id + 1001,
       sort_order, NULL, NULL, FALSE, NULL, NULL, NULL
FROM issues WHERE id = '27a64497-44aa-446b-8136-8197010dea71';

INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, parent_id, state_id, point, estimate_point_id,
   name, description_json, description_html, description_stripped,
   description_binary, priority, start_date, target_date, sequence_id,
   sort_order, completed_at, archived_at, is_draft, external_source,
   external_id, type_id)
SELECT '2026-03-01 00:00:02+00', '2026-03-01 00:00:02+00',
       'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
       '2026-03-02 00:00:00+00',
       'cc000000-0000-4000-8000-000000000012',
       project_id, workspace_id, NULL, state_id, NULL, NULL,
       'Harness deleted issue', description_json,
       '<p>Harness deleted issue</p>', 'Harness deleted issue', NULL,
       priority, NULL, NULL, sequence_id + 1002, sort_order, NULL, NULL,
       FALSE, NULL, NULL, NULL
FROM issues WHERE id = '27a64497-44aa-446b-8136-8197010dea71';

INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, parent_id, state_id, point, estimate_point_id,
   name, description_json, description_html, description_stripped,
   description_binary, priority, start_date, target_date, sequence_id,
   sort_order, completed_at, archived_at, is_draft, external_source,
   external_id, type_id)
VALUES
  ('2026-03-01 00:00:03+00', '2026-03-01 00:00:03+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'cc000000-0000-4000-8000-000000000013',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL,
   '2b498bca-5fb6-4a50-a4d3-30e2f4fffc64', NULL, NULL,
   'Harness beta member issue', '{"type": "doc", "content": [{"type": "paragraph"}]}',
   '<p>Harness beta member issue</p>', 'Harness beta member issue', NULL,
   'none', NULL, NULL, 5003, 300000, NULL, NULL, FALSE, NULL, NULL, NULL);

INSERT INTO issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, parent_id, state_id, point, estimate_point_id,
   name, description_json, description_html, description_stripped,
   description_binary, priority, start_date, target_date, sequence_id,
   sort_order, completed_at, archived_at, is_draft, external_source,
   external_id, type_id)
VALUES
  ('2026-03-01 00:00:04+00', '2026-03-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000014',
   '95100651-3264-4bd8-b2f7-5d6a082c9851',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL, NULL, NULL, NULL,
   'Harness zeta issue', '{"type": "doc", "content": [{"type": "paragraph"}]}',
   '<p>Harness zeta issue</p>', 'Harness zeta issue', NULL,
   'none', NULL, NULL, 5004, 310000, NULL, NULL, FALSE, NULL, NULL, NULL);

-- ---------------------------------------------------------------------------
-- Intake links for the three alpha issues above.
-- ---------------------------------------------------------------------------
INSERT INTO intake_issues
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, intake_id, issue_id, status, snoozed_till,
   duplicate_to_id, source, source_email, external_source, external_id, extra)
VALUES
  ('2026-03-01 00:01:00+00', '2026-03-01 00:01:00+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000020',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'cc000000-0000-4000-8000-000000000010', -2, NULL, NULL, 'IN_APP',
   NULL, NULL, NULL, '{}'),
  ('2026-03-01 00:01:01+00', '2026-03-01 00:01:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000021',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'cc000000-0000-4000-8000-000000000011', -2, NULL, NULL, 'IN_APP',
   NULL, NULL, NULL, '{}'),
  ('2026-03-01 00:01:02+00', '2026-03-01 00:01:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'cc000000-0000-4000-8000-000000000022',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '826885ac-00cd-47cd-b54a-77a52004021c',
   'cc000000-0000-4000-8000-000000000012', -2, NULL, NULL, 'IN_APP',
   NULL, NULL, NULL, '{}');
