-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `scenarios/write/auth-oauth-google-github.yaml`.
--
-- Configures Google and GitHub (the two providers with hardcoded URLs) and
-- sets `GITHUB_ORGANIZATION_ID`, so every GitHub callback also exercises the
-- organization-membership gate. The outbound calls are intercepted by
-- `tools/auth/oauth_https_proxy.py`; the client id/secret values only have to
-- be non-empty. The rows are seeded `""` by
-- `plane/utils/instance_config_variables/core.py`, and a database row beats
-- the environment (`ENV.md`), so an UPDATE is enough.
--
-- The template already seeds one `accounts` row per provider
-- (`tools/fixtures/build.py:2647-2664`): `google-admin-account` for
-- `fix.admin@example.com` and `github-member-account` for
-- `fix.member@example.com`. The proxies' sign-in cases return exactly those
-- ids, so the callbacks take `create_update_account`'s update branch; the
-- sign-up cases create new rows.
--
-- Two accepted invitations let the app sign-ups join `fix-workspace`
-- (`post_user_auth_workflow`); the space callbacks never run it.

BEGIN;

-- The template's instance row is not set up, which turns every auth route into
-- the `INSTANCE_NOT_CONFIGURED` redirect before the provider is constructed.
UPDATE instances SET is_setup_done = true, updated_at = now();

INSERT INTO instance_configurations
    (id, created_at, updated_at, key, value, category, is_encrypted)
VALUES
    ('b4000000-0000-4000-8000-000000000001', now(), now(), 'GOOGLE_CLIENT_ID', 'fake-google-client-id', 'oauth', false),
    ('b4000000-0000-4000-8000-000000000002', now(), now(), 'GOOGLE_CLIENT_SECRET', 'fake-google-client-secret', 'oauth', false),
    ('b4000000-0000-4000-8000-000000000003', now(), now(), 'GITHUB_CLIENT_ID', 'fake-github-client-id', 'oauth', false),
    ('b4000000-0000-4000-8000-000000000004', now(), now(), 'GITHUB_CLIENT_SECRET', 'fake-github-client-secret', 'oauth', false),
    -- The membership check is per user: the proxy answers 200 for every login
    -- except `gh-outsider`, which is the non-member case.
    ('b4000000-0000-4000-8000-000000000005', now(), now(), 'GITHUB_ORGANIZATION_ID', 'fake-org-id', 'oauth', false),
    -- `check_sync_enabled` reads `== "1"`; the sign-in cases then re-apply the
    -- IDP's name and avatar, which is the `sync_user_data` path.
    ('b4000000-0000-4000-8000-000000000006', now(), now(), 'ENABLE_GOOGLE_SYNC', '1', 'oauth', false),
    ('b4000000-0000-4000-8000-000000000007', now(), now(), 'ENABLE_GITHUB_SYNC', '1', 'oauth', false)
ON CONFLICT (key) DO UPDATE
    SET value = EXCLUDED.value,
        is_encrypted = false,
        updated_at = now();

-- Both sign-in users own a `file_assets` row in `plane_fix_tpl`. The sync path
-- would try to delete it through `S3Storage(request=…)`, whose custom domain is
-- built from the request's `Host` (`plane/settings/storage.py:42-56`);
-- `plane-fixture.test` does not resolve, and botocore's legacy retry mode then
-- stalls the step for ~6 s on Django but not on Rust, which flips the
-- harness's 10 s cookie-expiry bucket. The avatar *upload* half is still
-- covered: both avatars are loopback URLs the pinned fetcher rejects, so the
-- URL is stored and `delete_old_avatar` is unit-tested.
UPDATE users SET avatar_asset_id = NULL
 WHERE email IN ('fix.member@example.com', 'fix.admin@example.com');

-- Accepted workspace invites for the two app sign-up addresses. The app
-- callback runs `post_user_auth_workflow` → `process_workspace_project_invitations`,
-- which bulk-creates the membership and soft-deletes the invite.
INSERT INTO workspace_member_invites
    (id, created_at, updated_at, email, accepted, token, message, responded_at,
     role, workspace_id)
VALUES
    ('b5000000-0000-4000-8000-000000000001', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     'oauth.google.signup@example.com', true, 'fixture-invite-token-google', NULL, '2026-01-01T00:00:00Z',
     5, 'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
    ('b5000000-0000-4000-8000-000000000002', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     'oauth.github.signup@example.com', true, 'fixture-invite-token-github', NULL, '2026-01-01T00:00:00Z',
     5, 'a91c3d80-5ed0-40f3-92a8-a28c015f4388')
ON CONFLICT (email, workspace_id, deleted_at) DO NOTHING;

COMMIT;
