-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- The Task-1 filter-gap fixture: a workspace whose issues carry one label
-- each and belong to one module, so `x_axis=labels__id` has label details and
-- `segment=issue_module__module_id` has a segment value. It pins three
-- `issue_filters(data, "POST")` behaviours:
--
--   1. a literal `"None"` in a UUID list is *not* `IS NULL` (that rule is GET
--      only); it reaches Django's `UUIDField.to_python` and aborts the task;
--   2. a JSON `null` item is discarded by Django's `In` lookup and an
--      all-null list is `EmptyResultSet` (a header-only CSV, still e-mailed);
--   3. `logged_by` raises `FieldError` only when its guard emits the lookup,
--      and the module-segment branch of `generate_segmented_rows` raises
--      `KeyError` when labels were requested too.
--
-- Each issue has at most one label: Django's label filter is a JOIN, and an
-- issue with two labels would be counted twice (see analytic_export.rs).
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES
    ('', 'e0000000-0000-4000-8000-000000000001', 'ada_gaps', 'ada-gaps@example.com',
     'Ada', 'Admin', '', '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Ada Admin', true, false);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'e0000000-0000-4000-8000-000000000010', 'P8C Gaps', 'p8c-gaps',
    'e0000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone, archived_at
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'e0000000-0000-4000-8000-000000000011', 'Gap Platform', '', 0, 'GAP',
    'e0000000-0000-4000-8000-000000000010', false, false, false, false,
    false, 30, 30, '{}', false, false, false, 'UTC', NULL
);

INSERT INTO project_members (
    created_at, updated_at, id, role, project_id, workspace_id, view_props,
    default_props, sort_order, preferences, is_active, member_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'e0000000-0000-4000-8000-000000000020', 20,
    'e0000000-0000-4000-8000-000000000011',
    'e0000000-0000-4000-8000-000000000010', '{}', '{}', 65535.0, '{}', true,
    'e0000000-0000-4000-8000-000000000001'
);

INSERT INTO states (
    created_at, updated_at, id, name, description, color, slug, project_id,
    workspace_id, sequence, "group", "default", is_triage
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000031', 'Todo', '', '#ff0000', 'todo',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', 1000, 'unstarted', true, false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000032', 'Done', '', '#00ff00', 'done',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', 2000, 'completed', false, false);

INSERT INTO labels (
    created_at, updated_at, id, name, description, color, sort_order,
    project_id, workspace_id, parent_id, deleted_at
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000041', 'bug', '', '#ff0000', 65535.0,
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL, NULL),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000042', 'ui', '', '#0000ff', 65535.0,
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL, NULL);

INSERT INTO modules (
    created_at, updated_at, id, name, description, description_text,
    description_html, status, view_props, sort_order, logo_props, project_id,
    workspace_id, deleted_at
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'e0000000-0000-4000-8000-000000000051', 'Core', '', NULL, NULL,
    'planned', '{}', 65535.0, '{}',
    'e0000000-0000-4000-8000-000000000011',
    'e0000000-0000-4000-8000-000000000010', NULL
);

INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft,
    state_id, created_by_id
) VALUES
    ('2026-01-02 10:00:00+00', '2026-01-02 10:00:00+00',
     'e0000000-0000-4000-8000-000000000101', 'Gap one', '{}', 'urgent', 1,
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'e0000000-0000-4000-8000-000000000031',
     'e0000000-0000-4000-8000-000000000001'),
    ('2026-01-02 11:00:00+00', '2026-01-02 11:00:00+00',
     'e0000000-0000-4000-8000-000000000102', 'Gap two', '{}', 'low', 2,
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'e0000000-0000-4000-8000-000000000032',
     'e0000000-0000-4000-8000-000000000001'),
    -- No label: still one of the workspace's issues, and in the module.
    ('2026-01-02 12:00:00+00', '2026-01-02 12:00:00+00',
     'e0000000-0000-4000-8000-000000000103', 'Gap three', '{}', 'high', 3,
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', '', 65535.0, false,
     'e0000000-0000-4000-8000-000000000031',
     'e0000000-0000-4000-8000-000000000001');

INSERT INTO issue_labels (
    created_at, updated_at, id, issue_id, label_id, project_id, workspace_id,
    deleted_at
) VALUES
    ('2026-01-02 10:00:00+00', '2026-01-02 10:00:00+00',
     'e0000000-0000-4000-8000-000000000111',
     'e0000000-0000-4000-8000-000000000101',
     'e0000000-0000-4000-8000-000000000041',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL),
    ('2026-01-02 11:00:00+00', '2026-01-02 11:00:00+00',
     'e0000000-0000-4000-8000-000000000112',
     'e0000000-0000-4000-8000-000000000102',
     'e0000000-0000-4000-8000-000000000042',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL);

INSERT INTO module_issues (
    created_at, updated_at, id, module_id, issue_id, project_id, workspace_id,
    deleted_at
) VALUES
    ('2026-01-02 10:00:00+00', '2026-01-02 10:00:00+00',
     'e0000000-0000-4000-8000-000000000121',
     'e0000000-0000-4000-8000-000000000051',
     'e0000000-0000-4000-8000-000000000101',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL),
    ('2026-01-02 11:00:00+00', '2026-01-02 11:00:00+00',
     'e0000000-0000-4000-8000-000000000122',
     'e0000000-0000-4000-8000-000000000051',
     'e0000000-0000-4000-8000-000000000102',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL),
    ('2026-01-02 12:00:00+00', '2026-01-02 12:00:00+00',
     'e0000000-0000-4000-8000-000000000123',
     'e0000000-0000-4000-8000-000000000051',
     'e0000000-0000-4000-8000-000000000103',
     'e0000000-0000-4000-8000-000000000011',
     'e0000000-0000-4000-8000-000000000010', NULL);

-- `get_email_configuration` rows; the harness rewrites EMAIL_PORT per side.
INSERT INTO instance_configurations (
    created_at, updated_at, id, key, value, category, is_encrypted
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000201', 'EMAIL_HOST', '127.0.0.1', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000202', 'EMAIL_PORT', '18418', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000203', 'EMAIL_USE_TLS', '0', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000204', 'EMAIL_USE_SSL', '0', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000205', 'EMAIL_HOST_USER', '', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000206', 'EMAIL_HOST_PASSWORD', '', 'email', false),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'e0000000-0000-4000-8000-000000000207', 'EMAIL_FROM', 'Team Plane <team@mailer.plane.so>', 'email', false)
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, is_encrypted = FALSE, deleted_at = NULL;
