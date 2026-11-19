-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `workspace_seed_task.workspace_seed`: one workspace with one human member.
-- The task creates its own bot user and copies the seed JSON under
-- `apps/api/plane/seeds/data` into the workspace, so the fixture must not
-- pre-create any project.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES (
    '', 'a0000000-0000-4000-8000-000000000001', 'ada_seed', 'ada-seed@example.com',
    'Ada', 'Admin', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    '2026-01-01 00:00:00+00', '', '',
    false, false, false, true, false, false, false, '', 'UTC',
    '', '', 'email', '', false, 'Ada Admin', true, false
);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'a0000000-0000-4000-8000-000000000010', 'P8C Seed Workspace', 'p8c-seed',
    'a0000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO workspace_members (
    created_at, updated_at, id, role, member_id, workspace_id, company_role,
    is_active, view_props, default_props, issue_props, explored_features,
    getting_started_checklist, tips
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'a0000000-0000-4000-8000-000000000020', 20,
    'a0000000-0000-4000-8000-000000000001',
    'a0000000-0000-4000-8000-000000000010', '', true,
    '{}', '{}', '{}', '{}', '{}', '{}'
);
