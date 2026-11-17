-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Webhook write-scenario fixture. Applied to both differential targets before
-- the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are not
-- step diffs until a request changes them.
--
-- Why it exists (`plans/WEBHOOKS.md` §4, "Fixture"):
--
--   * `plane_fix_tpl` seeds three live webhooks, all in `fix-workspace` and all
--     created by `admin` (`tools/fixtures/build.py:2672-2740`). None is
--     soft-deleted, `fix-second` has none, and no row has a NULL `created_by_id`,
--     so three behaviours are unreachable without seeded rows:
--   * the live-row uniqueness is the **partial** index
--     `webhook_url_unique_url_when_deleted_at_null`
--     (`plane/db/models/webhook.py:56-62`): a soft-deleted twin of a live URL
--     proves that the DELETE's soft delete frees the URL and that a re-create
--     is 201, while a POST of the URL a *live* row holds is the 409;
--   * uniqueness is per `(workspace, url)`, not per URL: the `fix-second` row
--     holds the same URL as `fix-workspace`'s active webhook, so a POST of it
--     into `fix-second` is a 409 and one into `fix-workspace` is not;
--   * `created_by_id IS NULL` renders `created_by: null` in the 17-key body and
--     is what a full save rewrites verbatim (the column is a writable FK in the
--     serializer, `SERIALIZERS.md` §WebhookSerializer).
--
-- Ids are fixed and documented here; the scenarios' `vars` repeat them. The
-- `secret_key` values follow the generator's shape
-- (`plane_wh_` + 32 lowercase hex, `plane/db/models/webhook.py:17-18`) so the
-- normalizer's `webhook_secret` shape check accepts them; they are literal
-- fixture values, identical on both targets.
--
--   workspace          a91c3d80-5ed0-40f3-92a8-a28c015f4388  (fix-workspace)
--   workspace ws2      7045c0d6-5260-494d-a205-4cfbaab24fea  (fix-second)
--   admin              d44ae570-3de4-4413-af08-f686bf5b7235
--   webhook active     f8cfd225-6f45-442e-a86f-ec8317215958  (https://hooks.example.test/active)
--   webhook inactive   b3167eff-f419-4108-8ed1-ed025c0a9d16  (https://hooks.example.test/inactive)
--   webhook internal   9cffbfd4-5436-4466-b63c-771c4d2291d4  (https://api.example.test/internal)
--
-- `created_at` is older than every rich-fixture row (base time
-- 2026-11-16T00:00:00Z) so the list's `-created_at` order stays deterministic
-- and these rows sort last; each value is distinct because the ordering has no
-- tiebreaker.

-- ---------------------------------------------------------------------------
-- Webhooks
-- ---------------------------------------------------------------------------
INSERT INTO webhooks
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, url, is_active, secret_key, project, issue, module, cycle,
   issue_comment, is_internal, version)
VALUES
  -- The soft-deleted twin of the live active webhook's URL. The partial index
  -- ignores it, so the live row keeps the URL and a re-create of the same URL
  -- after a DELETE is a 201.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:09+00',
   'b9000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'https://hooks.example.test/active', TRUE,
   'plane_wh_00000000000000000000000000000001',
   FALSE, TRUE, FALSE, FALSE, FALSE, FALSE, 'v1'),
  -- A live row in fix-second holding fix-workspace's active URL: uniqueness is
  -- per workspace, so only a POST into fix-second conflicts.
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000002',
   '7045c0d6-5260-494d-a205-4cfbaab24fea',
   'https://hooks.example.test/active', TRUE,
   'plane_wh_00000000000000000000000000000002',
   TRUE, FALSE, FALSE, FALSE, FALSE, FALSE, 'v1'),
  -- No audit user: the 17-key create/regenerate shape renders
  -- `created_by: null`, and a full save writes NULL back.
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   NULL, NULL, NULL,
   'b9000000-0000-4000-8000-000000000003',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'https://hooks.example.test/orphan', TRUE,
   'plane_wh_00000000000000000000000000000003',
   FALSE, FALSE, FALSE, FALSE, FALSE, FALSE, 'v1');
