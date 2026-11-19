-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `get_asset_object_metadata`: an uploaded attachment whose object exists, an
-- attachment whose object was never uploaded (HEAD 404 -> `storage_metadata`
-- NULL) and a soft-deleted row the manager hides.
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
INSERT INTO projects (
    created_at, updated_at, id, name, description, network, identifier,
    workspace_id, cycle_view, module_view, issue_views_view, page_view,
    intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled,
    is_issue_type_enabled, guest_view_all_features, timezone
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0000000-0000-4000-8000-000000000003', 'P8C', '', 0, 'P8C',
    'c0000000-0000-4000-8000-000000000002', false, false, false, false,
    false, 30, 30, '{}', false, false, false, 'UTC'
);
INSERT INTO issues (
    created_at, updated_at, id, name, description_json, priority, sequence_id,
    project_id, workspace_id, description_html, sort_order, is_draft
) VALUES (
    '2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
    'c0000000-0000-4000-8000-000000000004', 'P8C issue', '{}', 'none', 1,
    'c0000000-0000-4000-8000-000000000003',
    'c0000000-0000-4000-8000-000000000002', '', 1.0, false
);
INSERT INTO file_assets (
    created_at, updated_at, id, attributes, asset, workspace_id, is_deleted,
    deleted_at, is_archived, entity_type, is_uploaded, size, storage_metadata,
    project_id, issue_id, entity_identifier
) VALUES
    -- Object exists: the task fills storage_metadata.
    ('2026-11-10 00:00:00+00', '2026-11-10 00:00:00+00',
     'c0000000-0000-4000-8000-000000000030',
     '{"name":"meta.txt","type":"text/plain","size":5}', 'p8c/meta.txt',
     'c0000000-0000-4000-8000-000000000002', false, NULL, false,
     'ISSUE_ATTACHMENT', true, 5, NULL,
     'c0000000-0000-4000-8000-000000000003',
     'c0000000-0000-4000-8000-000000000004',
     'c0000000-0000-4000-8000-000000000004'),
    -- Object missing: HEAD 404 writes SQL NULL.
    ('2026-11-10 00:00:00+00', '2026-11-10 00:00:00+00',
     'c0000000-0000-4000-8000-000000000031',
     '{"name":"missing.txt","type":"text/plain","size":5}', 'p8c/missing.txt',
     'c0000000-0000-4000-8000-000000000002', false, NULL, false,
     'ISSUE_ATTACHMENT', false, 5, '{"ContentType":"stale"}',
     'c0000000-0000-4000-8000-000000000003',
     'c0000000-0000-4000-8000-000000000004',
     'c0000000-0000-4000-8000-000000000004'),
    -- Soft-deleted: `FileAsset.objects.get` raises DoesNotExist.
    ('2026-11-10 00:00:00+00', '2026-11-10 00:00:00+00',
     'c0000000-0000-4000-8000-000000000032',
     '{"name":"gone.txt","type":"text/plain","size":5}', 'p8c/gone-meta.txt',
     'c0000000-0000-4000-8000-000000000002', false, '2026-11-11 00:00:00+00', false,
     'ISSUE_ATTACHMENT', true, 5, '{"ContentType":"stale"}',
     'c0000000-0000-4000-8000-000000000003',
     'c0000000-0000-4000-8000-000000000004',
     'c0000000-0000-4000-8000-000000000004');
