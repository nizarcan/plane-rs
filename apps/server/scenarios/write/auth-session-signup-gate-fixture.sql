-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `auth-session-signup-gate.yaml`: `ENABLE_SIGNUP=0`, one accepted
-- invitation for `invited@example.com`, one uppercase invitation and one
-- accepted space invitation.
--
-- Ids are `a1000000-…`, every timestamp is fixed, and the password hash is
-- `pbkdf2_sha256$1000000$authsessionfixturesalt$…` for
-- `Auth-Fixture-Passw0rd-2026!` (a `password_vars` entry), so the rows are
-- identical on both sides and outside the harness's "now" window.

UPDATE instances SET is_setup_done = true;


INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-00000000000d', '6f895582185dba6ccb72ae8539890b28', NULL, 'owner@example.com', 'Auth Fixture Owner',
  'Fixture', 'owner', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, true, false, true,
  false, false, 'fd36602939aeb6d3b769c66708950d22fd36602939aeb6d3b769c66708950d22', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);

UPDATE instance_configurations SET value = '0' WHERE key = 'ENABLE_SIGNUP';
INSERT INTO workspaces
  (created_at, updated_at, id, name, logo, slug, created_by_id, owner_id,
   updated_by_id, organization_size, deleted_at, logo_asset_id, timezone,
   background_color)
VALUES ('2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00',
  'b1000000-0000-4000-8000-000000000001', 'Invite Workspace', NULL,
  'invite-workspace', 'a1000000-0000-4000-8000-00000000000d',
  'a1000000-0000-4000-8000-00000000000d', NULL, NULL, NULL, NULL, 'UTC',
  '#abcdef');

INSERT INTO workspace_member_invites
  (created_at, updated_at, id, email, accepted, token, message, responded_at,
   role, created_by_id, updated_by_id, workspace_id, deleted_at)
VALUES ('2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00',
  'b1000000-0000-4000-8000-000000000002', 'invited@example.com', true,
  'invite-token-fixture', NULL, '2026-01-01T00:00:00+00:00', 5,
  'a1000000-0000-4000-8000-00000000000d', NULL,
  'b1000000-0000-4000-8000-000000000001', NULL);

-- A stored uppercase invitation: the adapter's `__check_signup` lookup is
-- exact (`adapter/base.py:108`) while the sign-up view lowercases its input, so
-- a lowercase sign-up for this address is SIGNUP_DISABLED.
INSERT INTO workspace_member_invites
  (created_at, updated_at, id, email, accepted, token, message, responded_at,
   role, created_by_id, updated_by_id, workspace_id, deleted_at)
VALUES ('2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00',
  'b1000000-0000-4000-8000-000000000003', 'Mixed@Example.com', false,
  'invite-token-mixed', NULL, NULL, 5,
  'a1000000-0000-4000-8000-00000000000d', NULL,
  'b1000000-0000-4000-8000-000000000001', NULL);

-- The space sign-up has no `post_user_auth_workflow` callback, so this accepted
-- invitation must stay live and no membership may appear.
INSERT INTO workspace_member_invites
  (created_at, updated_at, id, email, accepted, token, message, responded_at,
   role, created_by_id, updated_by_id, workspace_id, deleted_at)
VALUES ('2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00',
  'b1000000-0000-4000-8000-000000000004', 'invited-space@example.com', true,
  'invite-token-space', NULL, '2026-01-01T00:00:00+00:00', 15,
  'a1000000-0000-4000-8000-00000000000d', NULL,
  'b1000000-0000-4000-8000-000000000001', NULL);

