-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `delete_unuploaded_file_asset`: four old rows that differ in `is_uploaded`
-- and in the soft-delete flag, plus one recent unuploaded row. Only the old
-- unuploaded live row may disappear.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES (
    '', 'c0000000-0000-4000-8000-000000000001', 'p8c', 'p8c@example.com',
    '', '', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    '2026-01-01 00:00:00+00', '', '',
    false, false, false, true, false, false, false, '', 'UTC',
    '', '', 'email', '', false, '', true, false
);
INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0000000-0000-4000-8000-000000000002', 'P8C', 'p8c-ws',
    'c0000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);
INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, workspace_id, is_deleted,
    deleted_at, is_archived, entity_type, is_uploaded, size, storage_metadata
) VALUES
    -- Old, unuploaded, live: the only row the task deletes.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000010', '{"name":"old.txt"}', 'p8c/old.txt',
     'c0000000-0000-4000-8000-000000000002', false, NULL, false, 'ISSUE_ATTACHMENT',
     false, 1, NULL),
    -- Old but uploaded: kept.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000011', '{"name":"uploaded.txt"}', 'p8c/uploaded.txt',
     'c0000000-0000-4000-8000-000000000002', false, NULL, false, 'ISSUE_ATTACHMENT',
     true, 1, NULL),
    -- Recent and unuploaded: kept.
    ('2026-11-18 00:00:00+00', '2026-11-18 00:00:00+00',
     'c0000000-0000-4000-8000-000000000012', '{"name":"new.txt"}', 'p8c/new.txt',
     'c0000000-0000-4000-8000-000000000002', false, NULL, false, 'ISSUE_ATTACHMENT',
     false, 1, NULL),
    -- Old and unuploaded but soft-deleted: the default manager hides it.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000013', '{"name":"gone.txt"}', 'p8c/gone.txt',
     'c0000000-0000-4000-8000-000000000002', false, '2026-11-02 00:00:00+00', false,
     'ISSUE_ATTACHMENT', false, 1, NULL);
