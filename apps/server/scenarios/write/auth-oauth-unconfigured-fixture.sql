-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `scenarios/write/auth-oauth-unconfigured.yaml`: no provider is
-- configured except Gitea, whose host has no scheme. `GiteaOAuthProvider`
-- rejects that with `GITEA_NOT_CONFIGURED` **after** the credential check
-- (`provider/oauth/gitea.py:48-56`), which is the one error branch a missing
-- credential cannot reach.
--
-- `plane_fix_tpl` seeds every OAuth row `""`, and a database row beats the
-- environment, so the template alone is "unconfigured" for the other three.

BEGIN;

-- The template's instance row is not set up, which turns every auth route into
-- the `INSTANCE_NOT_CONFIGURED` redirect before the provider is constructed.
UPDATE instances SET is_setup_done = true, updated_at = now();

INSERT INTO instance_configurations
    (id, created_at, updated_at, key, value, category, is_encrypted)
VALUES
    ('b1000000-0000-4000-8000-000000000011', now(), now(), 'GITEA_HOST', 'gitea.example.test', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000012', now(), now(), 'GITEA_CLIENT_ID', 'fake-gitea-client-id', 'oauth', false),
    ('b1000000-0000-4000-8000-000000000013', now(), now(), 'GITEA_CLIENT_SECRET', 'fake-gitea-client-secret', 'oauth', false)
ON CONFLICT (key) DO UPDATE
    SET value = EXCLUDED.value,
        is_encrypted = false,
        updated_at = now();

COMMIT;
