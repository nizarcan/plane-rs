-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `scenarios/read/v1-assets-labels-states-intake.yaml`, applied to
-- both targets before the baseline snapshot (`vars: { fixture_sql: … }`).
--
-- Why it exists: `plane_fix_tpl` has no uploaded=false generic asset and no
-- row that only the legacy app routes soft-deleted (`is_deleted = true` with
-- `deleted_at IS NULL`, `plane/app/views/asset/base.py:63`), and both are what
-- the v1 generic-asset GET distinguishes.
--
-- Ids (the `b1000000-…` block):
--
--   file_assets  b1000000-0000-4000-8000-000000000001  alpha, uploaded=false
--   file_assets  b1000000-0000-4000-8000-000000000002  alpha, is_deleted only
--
-- Alpha's workspace id is a91c3d80-5ed0-40f3-92a8-a28c015f4388 and its project
-- id b0063033-54f9-49d8-b5f3-6b7a1551c6a6 in `plane_fix_tpl`.

INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
    workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
    external_id, external_source, is_uploaded, issue_id, page_id, project_id,
    size, storage_metadata, user_id, draft_issue_id, entity_identifier
) VALUES (
    '2026-01-04 00:00:00+00', '2026-01-04 00:00:00+00',
    'b1000000-0000-4000-8000-000000000001',
    '{"name": "pending.pdf", "size": 1024, "type": "application/pdf"}',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388/0badf00d-pending.pdf',
    'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388', false, NULL, false, NULL,
    'ISSUE_ATTACHMENT', NULL, NULL, false, NULL, NULL,
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 1024, '{}', NULL, NULL, NULL
);

INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
    workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
    external_id, external_source, is_uploaded, issue_id, page_id, project_id,
    size, storage_metadata, user_id, draft_issue_id, entity_identifier
) VALUES (
    '2026-01-04 00:00:00+00', '2026-01-04 00:00:00+00',
    'b1000000-0000-4000-8000-000000000002',
    '{"name": "legacy.pdf", "size": 2048, "type": "application/pdf"}',
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388/0badf00e-legacy.pdf',
    'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388', true, NULL, false, NULL,
    'ISSUE_ATTACHMENT', NULL, NULL, true, NULL, NULL,
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 2048, '{}', NULL, NULL, NULL
);
