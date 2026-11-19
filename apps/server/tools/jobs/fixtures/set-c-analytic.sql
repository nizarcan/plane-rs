-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `analytic_export_task`: a workspace with three visible states (two work items
-- in Todo, one in Done, one in Backlog) plus one issue in each excluded class
-- (draft, triage state, archived project, project without membership), two
-- estimate points, and the seven e-mail configuration rows the harness points
-- at its fake SMTP listener.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES
    ('', 'f0000000-0000-4000-8000-000000000001', 'ada_analytic', 'ada-analytic@example.com',
     'Ada', 'Admin', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Ada Admin', true, false),
    ('', 'f0000000-0000-4000-8000-000000000002', 'mia_analytic', 'mia-analytic@example.com',
     'Mia', 'Member', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Mia Member', true, false);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'f0000000-0000-4000-8000-000000000010', 'P8C Analytic', 'p8c-analytic',
    'f0000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone, archived_at
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000011', 'Alpha Platform', '', 0, 'ALP',
     'f0000000-0000-4000-8000-000000000010', false, false, false, false,
     false, 30, 30, '{}', false, false, false, 'UTC', NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000012', 'Secret', '', 0, 'SEC',
     'f0000000-0000-4000-8000-000000000010', false, false, false, false,
     false, 30, 30, '{}', false, false, false, 'UTC', NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000013', 'Archived', '', 0, 'ARC',
     'f0000000-0000-4000-8000-000000000010', false, false, false, false,
     false, 30, 30, '{}', false, false, false, 'UTC', '2026-02-01 00:00:00+00');

INSERT INTO project_members (
    created_at, updated_at, id, role, project_id, workspace_id, view_props,
    default_props, sort_order, preferences, is_active, member_id
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000020', 20,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '{}', '{}', 65535.0, '{}', true,
     'f0000000-0000-4000-8000-000000000001'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000021', 20,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '{}', '{}', 65535.0, '{}', true,
     'f0000000-0000-4000-8000-000000000002'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000022', 20,
     'f0000000-0000-4000-8000-000000000013',
     'f0000000-0000-4000-8000-000000000010', '{}', '{}', 65535.0, '{}', true,
     'f0000000-0000-4000-8000-000000000001');

INSERT INTO states (
    created_at, updated_at, id, name, description, color, slug, project_id,
    workspace_id, sequence, "group", "default", is_triage
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000031', 'Todo', '', '#ff0000', 'todo',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', 1000, 'unstarted', true, false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000032', 'Done', '', '#00ff00', 'done',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', 2000, 'completed', false, false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000033', 'Backlog', '', '#cccccc', 'backlog',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', 1000, 'backlog', false, false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000034', 'Triage', '', '#0000ff', 'triage',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', 3000, 'triage', false, true),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000035', 'Todo', '', '#ff0000', 'todo',
     'f0000000-0000-4000-8000-000000000013',
     'f0000000-0000-4000-8000-000000000010', 1000, 'unstarted', true, false);

INSERT INTO estimates (
    created_at, updated_at, id, name, description, project_id, workspace_id,
    type, last_used
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'f0000000-0000-4000-8000-000000000040', 'Points', '',
    'f0000000-0000-4000-8000-000000000011',
    'f0000000-0000-4000-8000-000000000010', 'points', true
);

INSERT INTO estimate_points (
    created_at, updated_at, id, key, description, value, estimate_id,
    project_id, workspace_id
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000041', 0, '', '5',
     'f0000000-0000-4000-8000-000000000040',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000042', 1, '', '8',
     'f0000000-0000-4000-8000-000000000040',
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010');

INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft,
    state_id, created_by_id, estimate_point_id
) VALUES
    ('2026-01-02 10:00:00+00', '2026-01-02 10:00:00+00',
     'f0000000-0000-4000-8000-000000000101', 'Alpha one', '{}', 'urgent', 1,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000031',
     'f0000000-0000-4000-8000-000000000001',
     'f0000000-0000-4000-8000-000000000041'),
    ('2026-01-02 11:00:00+00', '2026-01-02 11:00:00+00',
     'f0000000-0000-4000-8000-000000000102', 'Alpha two', '{}', 'low', 2,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000031',
     'f0000000-0000-4000-8000-000000000001',
     'f0000000-0000-4000-8000-000000000042'),
    ('2026-01-02 12:00:00+00', '2026-01-02 12:00:00+00',
     'f0000000-0000-4000-8000-000000000103', 'Alpha three', '{}', 'high', 3,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000032',
     'f0000000-0000-4000-8000-000000000001', NULL),
    ('2026-01-02 13:00:00+00', '2026-01-02 13:00:00+00',
     'f0000000-0000-4000-8000-000000000104', 'Alpha four', '{}', 'none', 4,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000033',
     'f0000000-0000-4000-8000-000000000001',
     'f0000000-0000-4000-8000-000000000041'),
    -- Draft: `issue_objects` hides it.
    ('2026-01-02 14:00:00+00', '2026-01-02 14:00:00+00',
     'f0000000-0000-4000-8000-000000000105', 'Alpha draft', '{}', 'medium', 5,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, true,
     'f0000000-0000-4000-8000-000000000031',
     'f0000000-0000-4000-8000-000000000001',
     'f0000000-0000-4000-8000-000000000042'),
    -- Triage state: hidden.
    ('2026-01-02 15:00:00+00', '2026-01-02 15:00:00+00',
     'f0000000-0000-4000-8000-000000000106', 'Alpha triage', '{}', 'medium', 6,
     'f0000000-0000-4000-8000-000000000011',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000034',
     'f0000000-0000-4000-8000-000000000001', NULL),
    -- Archived project: hidden.
    ('2026-01-02 16:00:00+00', '2026-01-02 16:00:00+00',
     'f0000000-0000-4000-8000-000000000107', 'Archived one', '{}', 'urgent', 1,
     'f0000000-0000-4000-8000-000000000013',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000035',
     'f0000000-0000-4000-8000-000000000001',
     'f0000000-0000-4000-8000-000000000041'),
    -- Project without Ada's membership: hidden by the membership update.
    ('2026-01-02 17:00:00+00', '2026-01-02 17:00:00+00',
     'f0000000-0000-4000-8000-000000000108', 'Secret one', '{}', 'urgent', 1,
     'f0000000-0000-4000-8000-000000000012',
     'f0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'f0000000-0000-4000-8000-000000000031',
     'f0000000-0000-4000-8000-000000000002', NULL);

-- `get_email_configuration` rows; the harness rewrites EMAIL_PORT per side.
INSERT INTO instance_configurations (
    created_at, updated_at, id, key, value, category, is_encrypted
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000201', 'EMAIL_HOST', '127.0.0.1', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000202', 'EMAIL_PORT', '18418', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000203', 'EMAIL_USE_TLS', '0', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000204', 'EMAIL_USE_SSL', '0', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000205', 'EMAIL_HOST_USER', '', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000206', 'EMAIL_HOST_PASSWORD', '', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'f0000000-0000-4000-8000-000000000207', 'EMAIL_FROM', 'Team Plane <team@mailer.plane.so>', 'email', false)
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, is_encrypted = FALSE, deleted_at = NULL;
