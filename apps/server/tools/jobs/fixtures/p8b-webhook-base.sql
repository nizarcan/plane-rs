-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- The webhook chain's fixture (set B): two active webhooks with every event
-- flag, one inactive and one soft-deleted (both must be ignored by
-- `webhook_activity`), a user with an avatar and one without, a project, an
-- unstarted state, an issue with one label and one assignee, a comment, a
-- cycle and a module with a member, and an intake row.
--
-- Every `webhooks.url` uses `{{http}}`; `difftest task` substitutes its
-- recorder's base URL into the setup SQL (and the kwargs) when the case has
-- an `http` block.
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES
    ('', 'b0000000-0000-4000-8000-000000000001', 'p8b-actor',
     'actor@example.com', 'Ada', 'Actor', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Ada Actor', true, false),
    ('', 'b0000000-0000-4000-8000-000000000002', 'p8b-receiver',
     'receiver@example.com', 'Rex', 'Receiver', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Rex Receiver', true, false),
    ('', 'b0000000-0000-4000-8000-000000000003', 'p8b-assignee',
     'assignee@example.com', 'Avi', 'Assignee', '/avatars/a.png',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Avi Assignee', true, false);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000010', 'P8B', 'p8b-ws',
    'b0000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000011', 'P8B project',
    'A project for the webhook cases.', 0, 'P8B',
    'b0000000-0000-4000-8000-000000000010', true, true, false, false,
    true, 30, 30, '{}', false, false, false, 'UTC'
);

INSERT INTO states (
    created_at, updated_at, id, name, description, color, slug, project_id,
    workspace_id, sequence, "group", "default", is_triage
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000012', 'Todo', '', '#60646c',
    'todo', 'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', 1.0, 'unstarted', true, false
);

INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft, state_id,
    point, created_by_id, updated_by_id, start_date, target_date, external_source,
    external_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
    'b0000000-0000-4000-8000-000000000013', 'Webhook issue', '{}', 'high', 7,
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '<p>body</p>', 1.0, false,
    'b0000000-0000-4000-8000-000000000012', 3,
    'b0000000-0000-4000-8000-000000000001',
    'b0000000-0000-4000-8000-000000000001', '2026-01-05', '2026-01-09',
    'github', 'ext-1'
);

INSERT INTO labels (
    created_at, updated_at, id, name, description, workspace_id, color, sort_order
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000014', 'bug', '', 
    'b0000000-0000-4000-8000-000000000010', '#ff0000', 65535
);

INSERT INTO issue_labels (
    created_at, updated_at, id, issue_id, label_id, project_id, workspace_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000015',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-000000000014',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010'
);

INSERT INTO issue_assignees (
    created_at, updated_at, id, assignee_id, issue_id, project_id, workspace_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000016',
    'b0000000-0000-4000-8000-000000000003',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010'
);

INSERT INTO issue_comments (
    created_at, updated_at, id, comment_stripped, attachments, issue_id,
    project_id, workspace_id, comment_html, comment_json, access, actor_id,
    edited_at
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000017', 'a comment', '{}',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '<p>a comment</p>', '{}',
    'INTERNAL', 'b0000000-0000-4000-8000-000000000001',
    '2026-01-03 00:00:00+00'
);

INSERT INTO cycles (
    created_at, updated_at, id, name, description, owned_by_id, project_id,
    workspace_id, view_props, sort_order, progress_snapshot, logo_props,
    timezone, version, start_date, end_date
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000018', 'Cycle 1', 'A cycle',
    'b0000000-0000-4000-8000-000000000001',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '{}', 65535.0, '{}', '{}',
    'UTC', 1, '2026-01-01 00:00:00+00', '2026-01-31 00:00:00+00'
);

INSERT INTO cycle_issues (
    created_at, updated_at, id, cycle_id, issue_id, project_id, workspace_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-000000000019',
    'b0000000-0000-4000-8000-000000000018',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010'
);

INSERT INTO modules (
    created_at, updated_at, id, name, description, status, project_id,
    workspace_id, view_props, sort_order, logo_props, lead_id, target_date
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-00000000001a', 'Module 1', 'A module',
    'in-progress', 'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '{}', 65535.0, '{}',
    'b0000000-0000-4000-8000-000000000001', '2026-02-01'
);

INSERT INTO module_members (
    created_at, updated_at, id, member_id, module_id, project_id, workspace_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-00000000001b',
    'b0000000-0000-4000-8000-000000000003',
    'b0000000-0000-4000-8000-00000000001a',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010'
);

INSERT INTO module_issues (
    created_at, updated_at, id, issue_id, module_id, project_id, workspace_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-00000000001c',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-00000000001a',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010'
);

INSERT INTO intakes (
    created_at, updated_at, id, name, description, is_default, view_props,
    project_id, workspace_id, logo_props
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-00000000001d', 'P8B Intake', '', true, '{}',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '{}'
);

INSERT INTO intake_issues (
    created_at, updated_at, id, status, intake_id, issue_id, project_id,
    workspace_id, extra, source, source_email, duplicate_to_id
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b0000000-0000-4000-8000-00000000001e', 0,
    'b0000000-0000-4000-8000-00000000001d',
    'b0000000-0000-4000-8000-000000000013',
    'b0000000-0000-4000-8000-000000000011',
    'b0000000-0000-4000-8000-000000000010', '{}', 'email',
    'sender@example.com', NULL
);

INSERT INTO webhooks (
    created_at, updated_at, id, url, is_active, secret_key, project, issue,
    module, cycle, issue_comment, workspace_id, is_internal, version,
    created_by_id
) VALUES
    ('2026-01-02 00:00:00+00', '2026-01-02 00:00:00+00',
     'b0000000-0000-4000-8000-000000000021', '{{http}}/hook-one', true,
     'whsec-p8b', true, true, true, true, true,
     'b0000000-0000-4000-8000-000000000010', false, 'v1',
     'b0000000-0000-4000-8000-000000000002'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'b0000000-0000-4000-8000-000000000022', '{{http}}/hook-two', true,
     '', true, true, true, true, true,
     'b0000000-0000-4000-8000-000000000010', false, 'v1',
     'b0000000-0000-4000-8000-000000000002'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'b0000000-0000-4000-8000-000000000023', '{{http}}/hook-inactive', false,
     '', true, true, true, true, true,
     'b0000000-0000-4000-8000-000000000010', false, 'v1',
     'b0000000-0000-4000-8000-000000000002'),
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'b0000000-0000-4000-8000-000000000024', '{{http}}/hook-deleted', true,
     '', true, true, true, true, true,
     'b0000000-0000-4000-8000-000000000010', false, 'v1',
     'b0000000-0000-4000-8000-000000000002'),
    -- A blocked target for the SSRF `ValueError` branch: no event flag, so
    -- `webhook_activity`'s flag filters never select it.
    ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     'b0000000-0000-4000-8000-000000000025', 'http://10.0.0.1/hook', true,
     '', false, false, false, false, false,
     'b0000000-0000-4000-8000-000000000010', false, 'v1',
     'b0000000-0000-4000-8000-000000000002');
UPDATE webhooks
   SET deleted_at = '2026-01-03 00:00:00+00'
 WHERE id = 'b0000000-0000-4000-8000-000000000024';
