-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Task 12/13 fixture (set B): four users (a receiver, two actors, one mention
-- target), one workspace with two projects/does-not-matter, two issues, and
-- `EmailNotificationLog` rows for the grouping and the e-mail body.
--
-- Actor ids sort in the same order the rows are inserted
-- (`…0001` before `…0003`), because the `plane_jobs` payload is `jsonb` and
-- sorts object keys: a multi-actor payload is compared in uuid order on the
-- Rust side and in insertion order on Django's (see the handler module docs).
INSERT INTO users (
    password, id, username, email, first_name, last_name, avatar,
    date_joined, created_at, updated_at, last_location, created_location,
    is_superuser, is_managed, is_password_expired, is_active, is_staff,
    is_email_verified, is_password_autoset, token, user_timezone,
    last_login_ip, last_logout_ip, last_login_medium, last_login_uagent,
    is_bot, display_name, is_email_valid, is_password_reset_required
) VALUES
    ('', 'b1000000-0000-4000-8000-000000000001', 'p8b-mail-actor',
     'actor@example.com', 'Ada', 'Actor', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Ada Actor', true, false),
    ('', 'b1000000-0000-4000-8000-000000000002', 'p8b-mail-receiver',
     'receiver@example.com', 'Rex', 'Receiver', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Rex Receiver', true, false),
    ('', 'b1000000-0000-4000-8000-000000000003', 'p8b-mail-actor-two',
     'avi@example.com', 'Avi', 'Assignee', '/avatars/avi.png',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Avi Assignee', true, false),
    ('', 'b1000000-0000-4000-8000-000000000004', 'p8b-mail-mention',
     'mona@example.com', 'Mona', 'Mentioned', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Mona Mentioned', true, false),
    ('', 'b1000000-0000-4000-8000-000000000005', 'p8b-mail-outsider',
     'outsider@example.com', 'Otto', 'Outsider', '',
     '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
     '2026-01-01 00:00:00+00', '', '',
     false, false, false, true, false, false, false, '', 'UTC',
     '', '', 'email', '', false, 'Otto Outsider', true, false);

INSERT INTO workspaces (
    created_at, updated_at, id, name, slug, owner_id, timezone, background_color
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b1000000-0000-4000-8000-000000000010', 'P8B mail', 'p8b-mail',
    'b1000000-0000-4000-8000-000000000001', 'UTC', '#ffffff'
);

INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b1000000-0000-4000-8000-000000000011', 'P8B mail project',
    'A project for the e-mail notification cases.', 0, 'P8B',
    'b1000000-0000-4000-8000-000000000010', true, true, false, false,
    true, 30, 30, '{}', false, false, false, 'UTC'
);

INSERT INTO states (
    created_at, updated_at, id, name, description, color, slug, project_id,
    workspace_id, sequence, "group", "default", is_triage
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'b1000000-0000-4000-8000-000000000012', 'Todo', '', '#60646c',
    'todo', 'b1000000-0000-4000-8000-000000000011',
    'b1000000-0000-4000-8000-000000000010', 1.0, 'unstarted', true, false
);

INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft, state_id,
    point, created_by_id, updated_by_id, start_date, target_date, external_source,
    external_id
) VALUES
    ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
     'b1000000-0000-4000-8000-000000000013', 'Email issue', '{}', 'high', 7,
     'b1000000-0000-4000-8000-000000000011',
     'b1000000-0000-4000-8000-000000000010', '<p>body</p>', 1.0, false,
     'b1000000-0000-4000-8000-000000000012', 3,
     'b1000000-0000-4000-8000-000000000001',
     'b1000000-0000-4000-8000-000000000001', '2026-01-05', '2026-01-09',
     NULL, NULL),
    ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
     'b1000000-0000-4000-8000-000000000023', 'Second issue', '{}', 'none', 8,
     'b1000000-0000-4000-8000-000000000011',
     'b1000000-0000-4000-8000-000000000010', '<p>body</p>', 1.0, false,
     'b1000000-0000-4000-8000-000000000012', NULL,
     'b1000000-0000-4000-8000-000000000001',
     'b1000000-0000-4000-8000-000000000001', NULL, NULL,
     NULL, NULL);

-- Unprocessed rows used by the grouping cases. Row order matters: the query
-- orders by receiver only, and these three rows are read in insertion order.
INSERT INTO email_notification_logs (
    created_at, updated_at, id, receiver_id, triggered_by_id,
    entity_identifier, entity_name, data, processed_at, sent_at, entity,
    old_value, new_value
) VALUES
    ('2026-01-02 09:05:00+00', '2026-01-02 09:05:00+00',
     'b1000000-0000-4000-8000-000000000031',
     'b1000000-0000-4000-8000-000000000002',
     'b1000000-0000-4000-8000-000000000001',
     'b1000000-0000-4000-8000-000000000013', 'issue',
     '{"issue": {"id": "b1000000-0000-4000-8000-000000000013", "name": "Email issue", "identifier": "P8B", "project_id": "b1000000-0000-4000-8000-000000000011", "workspace_slug": "p8b-mail", "sequence_id": 7, "state_name": "Todo", "state_group": "unstarted"}, "issue_activity": {"id": "b1000000-0000-4000-8000-000000000041", "verb": "updated", "field": "state", "actor": "b1000000-0000-4000-8000-000000000001", "new_value": "Done", "old_value": "Todo", "issue_comment": "", "old_identifier": null, "new_identifier": null, "activity_time": "2026-01-02T09:05:00Z"}}'::jsonb,
     NULL, NULL, 'issue', 'Todo', 'Done'),
    ('2026-01-02 09:06:00+00', '2026-01-02 09:06:00+00',
     'b1000000-0000-4000-8000-000000000032',
     'b1000000-0000-4000-8000-000000000002',
     'b1000000-0000-4000-8000-000000000003',
     'b1000000-0000-4000-8000-000000000013', 'issue',
     '{"issue": {"id": "b1000000-0000-4000-8000-000000000013", "name": "Email issue", "identifier": "P8B", "project_id": "b1000000-0000-4000-8000-000000000011", "workspace_slug": "p8b-mail", "sequence_id": 7, "state_name": "Todo", "state_group": "unstarted"}, "issue_activity": {"id": "b1000000-0000-4000-8000-000000000042", "verb": "updated", "field": "priority", "actor": "b1000000-0000-4000-8000-000000000003", "new_value": "urgent", "old_value": "low", "issue_comment": "", "old_identifier": null, "new_identifier": null, "activity_time": "2026-01-02T09:06:00Z"}}'::jsonb,
     NULL, NULL, 'issue', 'low', 'urgent'),
    ('2026-01-02 09:07:00+00', '2026-01-02 09:07:00+00',
     'b1000000-0000-4000-8000-000000000033',
     'b1000000-0000-4000-8000-000000000002',
     'b1000000-0000-4000-8000-000000000001',
     'b1000000-0000-4000-8000-000000000023', 'issue',
     '{"issue": {"id": "b1000000-0000-4000-8000-000000000023", "name": "Second issue", "identifier": "P8B", "project_id": "b1000000-0000-4000-8000-000000000011", "workspace_slug": "p8b-mail", "sequence_id": 8, "state_name": "Todo", "state_group": "unstarted"}, "issue_activity": {"id": "b1000000-0000-4000-8000-000000000043", "verb": "updated", "field": "name", "actor": "b1000000-0000-4000-8000-000000000001", "new_value": "Second issue", "old_value": "Old second", "issue_comment": "", "old_identifier": null, "new_identifier": null, "activity_time": "2026-01-02T09:07:00Z"}}'::jsonb,
     NULL, NULL, 'issue', 'Old second', 'Second issue'),
    -- Already processed: both grouping and the stamp must leave it alone.
    ('2026-01-01 08:00:00+00', '2026-01-01 08:00:00+00',
     'b1000000-0000-4000-8000-000000000034',
     'b1000000-0000-4000-8000-000000000002',
     'b1000000-0000-4000-8000-000000000001',
     'b1000000-0000-4000-8000-000000000013', 'issue',
     '{"issue": {"id": "b1000000-0000-4000-8000-000000000013"}, "issue_activity": {"id": "b1000000-0000-4000-8000-000000000044", "verb": "updated", "field": "name", "actor": "b1000000-0000-4000-8000-000000000001", "new_value": "Email issue", "old_value": "Old", "issue_comment": "", "old_identifier": null, "new_identifier": null, "activity_time": "2026-01-01T08:00:00Z"}}'::jsonb,
     '2026-01-01 08:01:00+00', NULL, 'issue', 'Old', 'Email issue');

-- The subject and body under escaping: control characters are stripped from
-- the subject (`remove_unwanted_characters`), HTML is escaped by the template.
UPDATE issues
   SET name = E'<script>alert(''x'')</script> & issue\u0001'
 WHERE id = 'b1000000-0000-4000-8000-000000000013';
