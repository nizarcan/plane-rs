-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `copy_s3_objects_of_description_and_assets`: a page and an issue whose
-- descriptions reference uploaded assets, plus the `file_assets` rows the task
-- duplicates. The asset list exercises the three filters in
-- `copy_assets`:
--
--   * two live assets in this project (one page, one issue description);
--   * one live asset in another project (excluded: project_id);
--   * one soft-deleted asset (excluded: deleted_at IS NULL).
--
-- `c0f0…0070` has no `name` in `attributes`, so its destination key ends in
-- `-None`; `c0f0…0072` is referenced by the issue.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES (
    '', 'c0f00000-0000-4000-8000-000000000001', 'ada_copy', 'ada-copy@example.com',
    'Ada', 'Admin', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    '2026-01-01 00:00:00+00', '', '',
    false, false, false, true, false, false, false, '', 'UTC',
    '', '', 'email', '', false, 'Ada Admin', true, false
);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0f00000-0000-4000-8000-000000000010', 'P8C Copy', 'p8c-copy',
    'c0f00000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone, archived_at
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000011', 'Copy Platform', '', 0, 'CPY',
     'c0f00000-0000-4000-8000-000000000010', false, false, false, false,
     false, 30, 30, '{}', false, false, false, 'UTC', NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000012', 'Other Project', '', 0, 'OTH',
     'c0f00000-0000-4000-8000-000000000010', false, false, false, false,
     false, 30, 30, '{}', false, false, false, 'UTC', NULL);

INSERT INTO project_members (
    created_at, updated_at, id, role, project_id, workspace_id, view_props,
    default_props, sort_order, preferences, is_active, member_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0f00000-0000-4000-8000-000000000020', 20,
    'c0f00000-0000-4000-8000-000000000011',
    'c0f00000-0000-4000-8000-000000000010', '{}', '{}', 65535.0, '{}', true,
    'c0f00000-0000-4000-8000-000000000001'
);

INSERT INTO states (
    created_at, updated_at, id, name, description, color, slug, project_id,
    workspace_id, sequence, "group", "default", is_triage
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000031', 'Todo', '', '#ff0000', 'todo',
     'c0f00000-0000-4000-8000-000000000011',
     'c0f00000-0000-4000-8000-000000000010', 1000, 'unstarted', true, false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000032', 'Done', '', '#00ff00', 'done',
     'c0f00000-0000-4000-8000-000000000011',
     'c0f00000-0000-4000-8000-000000000010', 2000, 'completed', false, false);

INSERT INTO pages (
    created_at, updated_at, id, name, description_json, description_html,
    description_stripped, access, created_by_id, owned_by_id, updated_by_id,
    workspace_id, color, is_locked, view_props, logo_props, is_global,
    sort_order
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0f00000-0000-4000-8000-000000000060', 'Copy source', '{}',
    '<p>Hello <mention-component id="c0f00000-0000-4000-8000-000000000001" name="Ada"></mention-component></p><p><image-component src="c0f00000-0000-4000-8000-000000000070" width="640"></image-component></p><p>External <image-component src="https://example.com/pic.png"></image-component></p><p><image-component src="c0f00000-0000-4000-8000-000000000073"></image-component></p>',
    NULL, 0, 'c0f00000-0000-4000-8000-000000000001',
    'c0f00000-0000-4000-8000-000000000001',
    'c0f00000-0000-4000-8000-000000000001',
    'c0f00000-0000-4000-8000-000000000010', '', false, '{}', '{}', false,
    65535.0
);

INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft,
    state_id, created_by_id, updated_by_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0f00000-0000-4000-8000-000000000061', 'Copy issue', '{}', 'none', 1,
    'c0f00000-0000-4000-8000-000000000011',
    'c0f00000-0000-4000-8000-000000000010',
    '<p><image-component src="c0f00000-0000-4000-8000-000000000072"></image-component></p>',
    65535.0, false, 'c0f00000-0000-4000-8000-000000000031',
    'c0f00000-0000-4000-8000-000000000001',
    'c0f00000-0000-4000-8000-000000000001'
);

INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
    workspace_id, is_deleted, is_archived, entity_type, is_uploaded,
    issue_id, page_id, project_id, size, storage_metadata, deleted_at
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000070',
     '{"name": "photo.png", "type": "image/png", "size": 1234}',
     'c0f00000-0000-4000-8000-000000000010/deadbeef-photo.png',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000010', false, false, 'PAGE_DESCRIPTION',
     true, NULL, 'c0f00000-0000-4000-8000-000000000060',
     'c0f00000-0000-4000-8000-000000000011', 1234.0,
     '{"ContentLength": 1234}', NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000072',
     '{"name": "issue.png", "type": "image/png", "size": 99}',
     'c0f00000-0000-4000-8000-000000000010/feedface-issue.png',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000010', false, false, 'ISSUE_DESCRIPTION',
     true, 'c0f00000-0000-4000-8000-000000000061', NULL,
     'c0f00000-0000-4000-8000-000000000011', 99.0, NULL, NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000071',
     '{"name": "other.png", "type": "image/png"}',
     'c0f00000-0000-4000-8000-000000000010/0badcafe-other.png',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000010', false, false, 'PAGE_DESCRIPTION',
     true, NULL, 'c0f00000-0000-4000-8000-000000000060',
     'c0f00000-0000-4000-8000-000000000012', 5.0, NULL, NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'c0f00000-0000-4000-8000-000000000073',
     '{"name": "deleted.png", "type": "image/png"}',
     'c0f00000-0000-4000-8000-000000000010/deadbeef-deleted.png',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000001',
     'c0f00000-0000-4000-8000-000000000010', false, FALSE, 'PAGE_DESCRIPTION',
     true, NULL, 'c0f00000-0000-4000-8000-000000000060',
     'c0f00000-0000-4000-8000-000000000011', 1.0, NULL,
     '2026-01-03 00:00:00+00');
