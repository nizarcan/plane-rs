-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Extra deploy boards for `scenarios/read/public-extra.yaml`. Both are loaded
-- into the target's clone of `plane_fix_tpl`, so the fixtures they reference
-- (workspace, projects, issues, assets) are the template's.
--
-- 1. A published **page** board. `entity_name = 'page'`, `entity_identifier`
--    is a page id that does not exist, while `project_id` is Alpha Platform.
--    `PUBLIC_INSTANCES.md` §5 records that such a board carries a project id,
--    which is why the asset and list routes (they look the board up without an
--    `entity_name` filter) answer that project's data.
-- 2. A project board whose `entity_identifier` is **not** its `project_id`.
--    `DeployBoardAnchorMixin.initial` (`plane/space/views/base.py:47-59`)
--    answers `{"detail": "Not found."}` for every route on this anchor before
--    a handler runs.

INSERT INTO deploy_boards (
    id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
    workspace_id, project_id, entity_identifier, entity_name, anchor,
    is_comments_enabled, is_reactions_enabled, intake_id, is_votes_enabled,
    view_props, is_activity_enabled, is_disabled
) VALUES (
    'e8d3b0d1-4c11-4a4e-9c39-1a2b3c4d5e6f',
    '2026-01-20 00:00:00+00', '2026-01-20 00:15:00+00', NULL, NULL, NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
    '9b1c2d3e-4f50-4a61-8b72-c93d4e5f6a7b',
    'page', 'extra-anchor-page',
    TRUE, TRUE, NULL, TRUE,
    '{}'::jsonb, TRUE, FALSE
);

INSERT INTO deploy_boards (
    id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
    workspace_id, project_id, entity_identifier, entity_name, anchor,
    is_comments_enabled, is_reactions_enabled, intake_id, is_votes_enabled,
    view_props, is_activity_enabled, is_disabled
) VALUES (
    'f9e4c1e2-5d22-4b5f-8d4a-2b3c4d5e6f70',
    '2026-01-21 00:00:00+00', '2026-01-21 00:15:00+00', NULL, NULL, NULL,
    'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
    'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
    '00000000-0000-4000-8000-00000000000a',
    'project', 'extra-anchor-mismatch',
    TRUE, TRUE, NULL, TRUE,
    '{}'::jsonb, TRUE, FALSE
);
