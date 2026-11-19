-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `delete_old_s3_link`: expired rows with a key, with an empty key and with a
-- NULL url; a fresh row; and a soft-deleted expired row. The S3 objects the
-- case seeds are `p8c/old.zip` (must go), `p8c/empty.zip` (no row points at it,
-- must stay) and `p8c/keep.zip` (fresh row, must stay).
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
INSERT INTO exporters (
    created_at, updated_at, id, project, provider, status, reason, key, url,
    token, initiated_by_id, workspace_id, filters, name, type, deleted_at,
    rich_filters
) VALUES
    -- Expired, with a key: delete the object and null the url.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000020', '{}', 'csv', 'completed', '',
     'p8c/old.zip', 'http://127.0.0.1:59000/uploads/p8c/old.zip', 'token-old',
     'c0000000-0000-4000-8000-000000000001',
     'c0000000-0000-4000-8000-000000000002', NULL, 'old', 'issue_exports', NULL,
     '{}'),
    -- Fresh: untouched.
    ('2026-11-18 00:00:00+00', '2026-11-18 00:00:00+00',
     'c0000000-0000-4000-8000-000000000021', '{}', 'csv', 'completed', '',
     'p8c/keep.zip', 'http://127.0.0.1:59000/uploads/p8c/keep.zip', 'token-keep',
     'c0000000-0000-4000-8000-000000000001',
     'c0000000-0000-4000-8000-000000000002', NULL, 'keep', 'issue_exports', NULL,
     '{}'),
    -- Expired but no url: not selected.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000022', '{}', 'csv', 'queued', '',
     '', NULL, 'token-nourl',
     'c0000000-0000-4000-8000-000000000001',
     'c0000000-0000-4000-8000-000000000002', NULL, 'nourl', 'issue_exports', NULL,
     '{}'),
    -- Expired, url set, empty key: url is nulled, no S3 call.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000023', '{}', 'csv', 'completed', '',
     '', 'http://127.0.0.1:59000/uploads/empty', 'token-empty',
     'c0000000-0000-4000-8000-000000000001',
     'c0000000-0000-4000-8000-000000000002', NULL, 'empty', 'issue_exports', NULL,
     '{}'),
    -- Soft-deleted and expired: the default manager hides it.
    ('2026-11-01 00:00:00+00', '2026-11-01 00:00:00+00',
     'c0000000-0000-4000-8000-000000000024', '{}', 'csv', 'completed', '',
     'p8c/gone.zip', 'http://127.0.0.1:59000/uploads/p8c/gone.zip', 'token-gone',
     'c0000000-0000-4000-8000-000000000001',
     'c0000000-0000-4000-8000-000000000002', NULL, 'gone', 'issue_exports',
     '2026-11-02 00:00:00+00', '{}');
