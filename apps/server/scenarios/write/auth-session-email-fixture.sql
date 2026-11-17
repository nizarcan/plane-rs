-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `auth-session-email.yaml`: the instance gate plus the accounts
-- the sign-in/sign-up branches need (active, deactivated, bot, provisioned,
-- existing).
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
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000001', '8ce2ce40be5d9febb473c3bfd00f3993', NULL, 'signin@example.com', 'Auth Fixture Signin',
  'Fixture', 'signin', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, true, false, true,
  false, false, '8238cd4062397a934e109d2e21866cdb8238cd4062397a934e109d2e21866cdb', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);

INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000002', 'db5f011ef372791d6cfd043b82516048', NULL, 'deactivated@example.com', 'Auth Fixture Deactivated',
  'Fixture', 'deactivated', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, false, false, true,
  false, false, '717e1f2a7ce35d013539c2823cdbddb6717e1f2a7ce35d013539c2823cdbddb6', '2026-01-01T00:00:00+00:00', NULL, '2026-01-01T00:00:00+00:00',
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);

INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000003', 'cea4625faf90d5bb719c3c3f6e78f9d1', NULL, 'bot@example.com', 'Auth Fixture Bot',
  'Fixture', 'bot', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, true, false, true,
  false, false, '0e548209f709d137f7b597b9308cfdb10e548209f709d137f7b597b9308cfdb1', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', true, NULL, 'UTC', false, NULL);

INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000004', 'acc320c8197369f5e0d0d54b471ca4cc', NULL, 'provisioned@example.com', 'Auth Fixture Provisioned',
  'Fixture', 'provisioned', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, false, false, true,
  false, false, '17b041a82de0b09ead7d82e55a151e6b17b041a82de0b09ead7d82e55a151e6b', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);

INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
VALUES ('pbkdf2_sha256$1000000$authsessionfixturesalt$EPjyFY5a91cT13fJwDhWin3mTglj/KBl4sNleXc0Sc8=', NULL, 'a1000000-0000-4000-8000-000000000005', '825016afb6696d0e7b2f226eb3754943', NULL, 'existing@example.com', 'Auth Fixture Existing',
  'Fixture', 'existing', '', NULL, NULL, NULL, '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00', '', '',
  false, false, false, true, false, true,
  false, false, 'd440d57695ad33b94ba4fae91846cc44d440d57695ad33b94ba4fae91846cc44', '2026-01-01T00:00:00+00:00', NULL, NULL,
  '', '', '', '', '2026-01-01T00:00:00+00:00', false, NULL, 'UTC', false, NULL);
