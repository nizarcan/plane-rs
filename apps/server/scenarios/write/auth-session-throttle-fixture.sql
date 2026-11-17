-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `auth-session-throttle.yaml`: the instance gate and one active
-- account; the scenario pins `AUTHENTICATION_RATE_LIMIT` itself.
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
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000010', '59003fef9dc3b5917762e057a65094ce', NULL, 'throttle@example.com', 'Auth Fixture Throttle',
  'Fixture', 'throttle', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, true, false, true,
  false, false, 'c6fccf910d7f33da13b183f1f8cea646c6fccf910d7f33da13b183f1f8cea646', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);
