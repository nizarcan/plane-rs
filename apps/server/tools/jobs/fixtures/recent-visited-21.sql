-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- The user and workspace every recent_visited_task case uses. The template
-- database has the schema but no rows, so this is the smallest fixture the
-- foreign keys allow. `project_id` stays NULL (a workspace-level view), which
-- exercises `IS NOT DISTINCT FROM` in the handler.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES (
    '', '11111111-1111-1111-1111-111111111111', 'p8p0', 'p8p0@example.com',
    '', '', '', now(), now(), now(), '', '',
    false, false, false, true, false, false, false, '', 'UTC',
    '', '', 'email', '', false, '', true, false
);
INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    now(), now(), '22222222-2222-2222-2222-222222222222', 'P8P0', 'p8p0-ws',
    '11111111-1111-1111-1111-111111111111', 'UTC', '#ffffff'
);

-- Twenty-one live visits: the upstream `count == 20` comparison never trims
-- again (`recent_visited_task.py:37-44`). The task must insert a 22nd row.
INSERT INTO user_recent_visits (
    id, created_at, updated_at, created_by_id, updated_by_id, deleted_at,
    workspace_id, project_id, entity_identifier, entity_name, user_id, visited_at
)
SELECT
    ('00000000-0000-0000-0000-' || lpad(n::text, 12, '0'))::uuid,
    now() - (n || ' hours')::interval,
    now() - (n || ' hours')::interval,
    '11111111-1111-1111-1111-111111111111',
    '11111111-1111-1111-1111-111111111111',
    NULL,
    '22222222-2222-2222-2222-222222222222',
    NULL,
    ('44444444-4444-4444-4444-' || lpad(n::text, 12, '0'))::uuid,
    'issue',
    '11111111-1111-1111-1111-111111111111',
    now() - (n || ' hours')::interval
FROM generate_series(1, 21) AS n;
