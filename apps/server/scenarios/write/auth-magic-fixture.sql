-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Magic-link fixture (`plans/AUTH_FLOWS.md` §4).
--
-- 1. `require_setup` gates every route in this set:
--    `instance is None or not instance.is_setup_done` → 400
--    `INSTANCE_NOT_CONFIGURED` (`views/app/magic.py:43-49`). The fixture
--    template ships `is_setup_done = FALSE`.
--
-- 2. The provider reads both keys from `instance_configurations`, and the seeded
--    rows win over the environment because `SKIP_ENV_VAR` defaults to `"1"`
--    (`plane/settings/common.py:401`, `license/utils/instance_value.py:19-33`).
--    The template seeds `EMAIL_HOST = ''` (SMTP not configured) and
--    `ENABLE_MAGIC_LINK_LOGIN = '0'` (login disabled) — `ENV.md` §5 — so both
--    must be flipped here; `target_env` cannot, a database row always wins.
--
-- 3. Auto-join: `post_user_auth_workflow` consumes *accepted* invites only
--    (`authentication/utils/workspace_project_join.py:15-27`). The template's
--    invites are all unaccepted, so the app variants would never join
--    anything. `fix.invitee` (an existing account, member of no workspace) and
--    `new-hire@example.com` (a brand-new signup) each get their existing
--    `fix-workspace` invite accepted. `fix.invitee`'s second invite, in
--    `fix-second`, stays unaccepted and proves the filter.
--
-- 4. The `is_password_autoset and profile.is_onboarded` branch sends an app
--    sign-in to `/?next_path=/` (`views/app/magic.py:112-114`); `fix.onboarded`
--    exercises it.
--
-- 5. `magic-project@example.com` gets an accepted workspace *and* project
--    invite. `process_workspace_project_invitations` inserts the `ProjectMember`
--    row without `project_id` (NOT NULL), so the signup is a 500 **after** the
--    workspace membership was committed (`workspace_project_join.py:55-65`).
--    Upstream bug, ported as-is; the step is last because it leaves rows.
--
-- `magic-dead@example.com` proves the deactivated-account gate: `is_active =
-- FALSE` with `last_logout_time` set is `USER_ACCOUNT_DEACTIVATED`
-- (`authentication/adapter/base.py:317-329`), which the template's
-- `fix.inactive` already is.
--
-- Ids are `a7b0…`; timestamps are fixed so both sides start identical and
-- outside the harness's "now" window.

UPDATE instances
SET is_setup_done = TRUE
WHERE deleted_at IS NULL;

UPDATE instance_configurations
SET value = '127.0.0.1'
WHERE key = 'EMAIL_HOST';

UPDATE instance_configurations
SET value = '1'
WHERE key = 'ENABLE_MAGIC_LINK_LOGIN';

-- 3. Accept the two invites the auto-join covers.
UPDATE workspace_member_invites
SET accepted = TRUE,
    responded_at = '2026-01-02 00:00:01+00'
WHERE email = 'fix.invitee@example.com'
  AND workspace_id = 'a91c3d80-5ed0-40f3-92a8-a28c015f4388'
  AND deleted_at IS NULL;

UPDATE workspace_member_invites
SET accepted = TRUE,
    responded_at = '2026-01-02 00:00:02+00'
WHERE email = 'new-hire@example.com'
  AND deleted_at IS NULL;

-- 5. A fresh address with an accepted workspace and project invite.
INSERT INTO workspace_member_invites
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, email, accepted, token, message, responded_at, role)
VALUES
  ('2026-01-02 00:00:03+00', '2026-01-02 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a7b00000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'magic-project@example.com', TRUE,
   'a7b0000000000000000000000000000000000000000000000000000000000001',
   NULL, '2026-01-02 00:00:03+00', 15);

INSERT INTO project_member_invites
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, email, accepted, token, message, responded_at,
   role)
VALUES
  ('2026-01-02 00:00:04+00', '2026-01-02 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a7b00000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 'magic-project@example.com', TRUE,
   'a7b0000000000000000000000000000000000000000000000000000000000002',
   NULL, '2026-01-02 00:00:04+00', 15);

-- 4. The `path = "/"` branch: a password-autoset account with an onboarded
-- profile.
UPDATE users
SET is_password_autoset = TRUE
WHERE email = 'fix.onboarded@example.com';

UPDATE profiles
SET is_onboarded = TRUE
WHERE user_id = (
  SELECT id FROM users WHERE email = 'fix.onboarded@example.com'
);

-- 6. A provisioned account that was never deactivated: `is_active = FALSE` with
-- `last_logout_time = NULL` passes the deactivation gate and is reactivated by
-- `save_user_data`, which then publishes `user_activation_email`
-- (`authentication/adapter/base.py:239-251`). It deliberately has **no**
-- profile row, so the app sign-in's `Profile.objects.get_or_create` takes its
-- INSERT branch (`views/app/magic.py:109`).
INSERT INTO users
  (password, last_login, id, username, mobile_number, email, display_name,
   first_name, last_name, avatar, avatar_asset_id, cover_image,
   cover_image_asset_id, date_joined, created_at, updated_at, last_location,
   created_location, is_superuser, is_managed, is_password_expired, is_active,
   is_staff, is_email_verified, is_password_autoset, is_password_reset_required,
   token, last_active, last_login_time, last_logout_time, last_login_ip,
   last_logout_ip, last_login_medium, last_login_uagent, token_updated_at,
   is_bot, bot_type, user_timezone, is_email_valid, masked_at)
SELECT
   password, NULL, 'a7b00000-0000-4000-8000-000000000010', 'a7b0provisioned',
   mobile_number, 'magic-provisioned@example.com', display_name, first_name,
   last_name, avatar, avatar_asset_id, cover_image, cover_image_asset_id,
   date_joined, created_at, updated_at, last_location, created_location,
   is_superuser, is_managed, is_password_expired, FALSE, is_staff,
   is_email_verified, is_password_autoset, is_password_reset_required,
   'a7b0000000000000000000000000000000000000000000000000000000000010',
   last_active, last_login_time, NULL, last_login_ip, last_logout_ip,
   last_login_medium, last_login_uagent, token_updated_at, is_bot, bot_type,
   user_timezone, is_email_valid, masked_at
FROM users
WHERE email = 'fix.admin@example.com';
