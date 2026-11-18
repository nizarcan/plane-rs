-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for the public-write scenarios (`scenarios/write/public-writes.yaml`
-- and `public-asset-writes.yaml`), applied to both targets before the baseline
-- snapshot (`vars: { fixture_sql: … }`), so these rows are not step diffs.
--
-- Why it exists:
--
-- * `fixture-anchor-gamma` publishes the archived Gamma project with an intake
--   and without any live triage state, so the public intake create's
--   `State.objects.create(name="Triage", …)` fallback runs and H19 sets
--   `sequence = MAX(non-triage 55000) + 15000 = 70000`. The rich fixture gives
--   every project a live Triage row (`tools/fixtures/build.py:753-822`) and
--   only Alpha has a board.
-- * Alpha's only EXTERNAL comments belong to the guest and the member, so the
--   member needs a `COMMENT_DESCRIPTION` asset of their own for the asset
--   PATCH/DELETE/bulk success paths, plus a soft-deleted one for restore
--   (`FileAsset.all_objects`).
--
-- Ids (all within the b2000000-… block so a failure is easy to grep):
--
--   intakes        b2000000-0000-4000-8000-000000000001  Gamma intake
--   deploy_boards  b2000000-0000-4000-8000-000000000002  fixture-anchor-gamma
--   file_assets    b2000000-0000-4000-8000-000000000011  member upload
--   file_assets    b2000000-0000-4000-8000-000000000012  member upload, soft-deleted
--
-- Alpha's workspace id is a91c3d80-5ed0-40f3-92a8-a28c015f4388 in `plane_fix_tpl`.

INSERT INTO intakes (
    created_at, updated_at, id, name, description, is_default, view_props,
    created_by_id, project_id, updated_by_id, workspace_id, logo_props, deleted_at
) VALUES (
    '2026-01-03 00:00:00+00', '2026-01-03 00:00:00+00',
    'b2000000-0000-4000-8000-000000000001', 'Gamma intake', '',
    false, '{}',
    'd44ae570-3de4-4413-af08-f686bf5b7235',
    '4f9d99bc-8083-4063-ac02-69dfb0e6ab95',
    'd44ae570-3de4-4413-af08-f686bf5b7235',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '{}', NULL
);

INSERT INTO deploy_boards (
    created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
    workspace_id, project_id, entity_identifier, entity_name, anchor,
    is_comments_enabled, is_reactions_enabled, intake_id, is_votes_enabled,
    view_props, is_activity_enabled, is_disabled
) VALUES (
    '2026-01-03 00:00:00+00', '2026-01-03 00:00:00+00',
    'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
    'b2000000-0000-4000-8000-000000000002',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
    '4f9d99bc-8083-4063-ac02-69dfb0e6ab95',
    '4f9d99bc-8083-4063-ac02-69dfb0e6ab95', 'project', 'fixture-anchor-gamma',
    true, true, 'b2000000-0000-4000-8000-000000000001', true,
    '{}', true, false
);

-- No live triage state in Gamma: the public create's fallback runs H19.
UPDATE states
   SET updated_at = '2026-01-02 00:00:00+00', deleted_at = '2026-01-02 00:00:00+00'
 WHERE id = 'eb67b03e-47c7-47cf-b444-bd15db243cb6'
   AND deleted_at IS NULL;

-- A member-owned comment image bound to the member's own EXTERNAL comment on
-- the commented Alpha issue (`d8fa6287-ddd7-4303-b743-02986ecbac94`).
INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
    workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
    external_id, external_source, is_uploaded, issue_id, page_id, project_id,
    size, storage_metadata, user_id, draft_issue_id, entity_identifier
) VALUES (
    '2026-01-03 00:00:00+00', '2026-01-03 00:00:00+00',
    'b2000000-0000-4000-8000-000000000011',
    '{"name": "member-upload.png", "size": 2048, "type": "image/png"}',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388/fixture-member-upload.png',
    '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388', false, NULL, false,
    'd8fa6287-ddd7-4303-b743-02986ecbac94', 'COMMENT_DESCRIPTION',
    NULL, NULL, true, NULL, NULL,
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 2048, '{}', NULL, NULL, NULL
);

INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
    workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
    external_id, external_source, is_uploaded, issue_id, page_id, project_id,
    size, storage_metadata, user_id, draft_issue_id, entity_identifier
) VALUES (
    '2026-01-03 00:00:00+00', '2026-01-03 00:00:00+00',
    'b2000000-0000-4000-8000-000000000012',
    '{"name": "member-deleted.png", "size": 1024, "type": "image/png"}',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388/fixture-member-deleted.png',
    '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388', true, '2026-01-04 00:00:00+00', false,
    NULL, 'COMMENT_DESCRIPTION',
    NULL, NULL, true, NULL, NULL,
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 1024, '{}', NULL, NULL, NULL
);
