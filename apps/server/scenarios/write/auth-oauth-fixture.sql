-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `scenarios/write/auth-oauth.yaml`.
--
-- Points the two self-hostable providers at `tools/auth/fake_provider.py`
-- (started on 127.0.0.1:18400 by `tools/auth/run-oauth-scenario.sh`) and
-- configures Google and GitHub with dummy credentials, so all eight initiate
-- views take their configured branch. The rows are seeded `""` by
-- `plane/utils/instance_config_variables/core.py`, and a database row beats the
-- environment (`ENV.md`), so an UPDATE is enough.
--
-- Also seeds the two `accounts` rows the sign-in cases upsert and one accepted
-- workspace invitation the GitLab sign-up joins (the app callback runs
-- `post_user_auth_workflow`; the space callback does not).

BEGIN;

-- The template's instance row is not set up, which turns every auth route into
-- the `INSTANCE_NOT_CONFIGURED` redirect before the provider is constructed.
UPDATE instances SET is_setup_done = true, updated_at = now();

INSERT INTO instance_configurations
    (id, created_at, updated_at, key, value, category, is_encrypted)
VALUES
    ('b1000000-0000-4000-8000-000000000001', now(), now(), 'GITLAB_HOST', 'http://127.0.0.1:18400', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000002', now(), now(), 'GITLAB_CLIENT_ID', 'fake-gitlab-client-id', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000003', now(), now(), 'GITLAB_CLIENT_SECRET', 'fake-gitlab-client-secret', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000004', now(), now(), 'GITEA_HOST', 'http://127.0.0.1:18400', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000005', now(), now(), 'GITEA_CLIENT_ID', 'fake-gitea-client-id', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000006', now(), now(), 'GITEA_CLIENT_SECRET', 'fake-gitea-client-secret', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000007', now(), now(), 'GOOGLE_CLIENT_ID', 'fake-google-client-id', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000008', now(), now(), 'GOOGLE_CLIENT_SECRET', 'fake-google-client-secret', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000009', now(), now(), 'GITHUB_CLIENT_ID', 'fake-github-client-id', 'oauth', false),
    ('b1000000-0000-4000-8000-00000000000a', now(), now(), 'GITHUB_CLIENT_SECRET', 'fake-github-client-secret', 'oauth', false),
    -- The IDP-sync cases need the flag on; `check_sync_enabled` reads `== "1"`.
    ('b1000000-0000-4000-8000-00000000000b', now(), now(), 'ENABLE_GITLAB_SYNC', '1', 'oauth', false),
    ('b1000000-0000-4000-8000-00000000000c', now(), now(), 'ENABLE_GITEA_SYNC', '1', 'oauth', false)
ON CONFLICT (key) DO UPDATE
    SET value = EXCLUDED.value,
        is_encrypted = false,
        updated_at = now();

-- `fix.member` carries a `file_assets` row in `plane_fix_tpl`. The sync path
-- would try to delete it through `S3Storage(request=…)`, whose custom domain is
-- built from the request's `Host` (`plane/settings/storage.py:42-56`);
-- `plane-fixture.test` does not resolve, and botocore's legacy retry mode then
-- stalls the step for ~6 s (five DNS attempts) on Django but not on Rust, which
-- flips the harness's 10 s cookie-expiry bucket. The avatar *upload* half is
-- still covered (the fake provider's avatar URL is rejected by the pinned
-- fetcher and stored as the URL), and `delete_old_avatar` is unit-tested.
UPDATE users SET avatar_asset_id = NULL WHERE email = 'fix.member@example.com';

-- `gitlab-signin` upserts this row: `create_update_account` matches on
-- (user, provider, provider_account_id) and writes a full row.
INSERT INTO accounts
    (id, created_at, updated_at, provider_account_id, provider, access_token,
     access_token_expired_at, refresh_token, refresh_token_expired_at,
     last_connected_at, metadata, user_id, id_token)
VALUES
    ('b2000000-0000-4000-8000-000000000001', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     '201', 'gitlab', 'fixture-old-gitlab-token',
     '2026-01-01T00:00:00Z', 'fixture-old-gitlab-refresh', '2026-01-01T00:00:00Z',
     '2026-01-01T00:00:00Z', '{}', '78100240-9cfd-4782-8627-25ab039c6e9a', ''),
    ('b2000000-0000-4000-8000-000000000002', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     '401', 'gitea', 'fixture-old-gitea-token',
     NULL, NULL, NULL,
     '2026-01-01T00:00:00Z', '{}', 'bc437914-c250-4c9f-ac69-85003fdf6205', '')
ON CONFLICT (provider, provider_account_id) DO NOTHING;

-- An accepted workspace invite for the GitLab sign-up address: the app callback
-- runs `post_user_auth_workflow` → `process_workspace_project_invitations`,
-- which bulk-creates the membership and soft-deletes the invite.
INSERT INTO workspace_member_invites
    (id, created_at, updated_at, email, accepted, token, message, responded_at,
     role, workspace_id)
VALUES
    ('b3000000-0000-4000-8000-000000000001', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     'oauth.gitlab.signup@example.com', true, 'fixture-invite-token-gitlab', NULL, '2026-01-01T00:00:00Z',
     5, 'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
    -- Not accepted: nothing is joined, but `get_redirection_path` still reports
    -- "invitations" for this address (the count ignores `accepted`).
    ('b3000000-0000-4000-8000-000000000002', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z',
     'oauth.gitea.signup@example.com', false, 'fixture-invite-token-gitea', NULL, NULL,
     5, '7045c0d6-5260-494d-a205-4cfbaab24fea')
ON CONFLICT (email, workspace_id, deleted_at) DO NOTHING;

COMMIT;
