-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- The auth/user e-mail tasks' fixture (set B): an invitor with a name, an
-- invitee with a name, a user whose first name and display name are empty (the
-- subject falls through to the e-mail), a workspace, a project, the project
-- membership row and one workspace invitation.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES
    ('', 'b0000000-0000-4000-8000-000000000101', 'p8b-invitor',
     'invitor@example.com', 'Ada', 'Invitor', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Ada Invitor', true, false),
    ('', 'b0000000-0000-4000-8000-000000000102', 'p8b-invitee',
     'invitee@example.com', 'Rex', 'Invitee', '/avatars/r.png',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Rex Invitee', true, false),
    ('', 'b0000000-0000-4000-8000-000000000103', 'p8b-nameless',
     'nameless@example.com', '', '', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, '', true, false);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000110', 'P8B Mail', 'p8b-mail',
    'b0000000-0000-4000-8000-000000000101', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000111', 'P8B Mail project', '', 0, 'P8M',
    'b0000000-0000-4000-8000-000000000110', false, false, false, false,
    false, 30, 30, '{}', false, false, false, 'UTC'
);

INSERT INTO project_members (
    created_at, updated_at, id, project_id, workspace_id, member_id, role,
    view_props, default_props, preferences, sort_order, is_active
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000112',
    'b0000000-0000-4000-8000-000000000111',
    'b0000000-0000-4000-8000-000000000110',
    'b0000000-0000-4000-8000-000000000102', 5,
    '{}', '{}', '{}', 65535, true
);

INSERT INTO workspace_member_invites (
    created_at, updated_at, id, workspace_id, email, accepted, token, message,
    responded_at, role, created_by_id, updated_by_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000113',
    'b0000000-0000-4000-8000-000000000110', 'invitee@example.com', false,
    'tok-1', NULL, NULL, 5,
    'b0000000-0000-4000-8000-000000000101',
    'b0000000-0000-4000-8000-000000000101'
);
