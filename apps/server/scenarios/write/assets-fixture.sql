-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Shared `file_assets` fixture for the three asset write sets
-- (`plans/ASSETS.md` §4, "Fixture gaps"). Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists — the rich fixture (`tools/fixtures/build.py:1556-1663`) has
-- only uploaded, live, admin-created rows of five entity types, so these
-- branches have no input without it:
--
--   * a **soft-deleted** asset (route 22 restore, `AssetRestoreEndpoint.post`
--     reads `all_objects`; the v1 deletes read the manager and must 404 it);
--   * a **USER_COVER** (routes 20/21 cover branch; the fixture has avatars
--     only). `member`'s `cover_image_asset` points at it, so the entity
--     helper's "delete the previous cover" branch is reachable too;
--   * a **PAGE_DESCRIPTION** and a **DRAFT_ISSUE_DESCRIPTION** for the bulk
--     rebind (route 26) — the fixture has ISSUE_/COMMENT_DESCRIPTION only;
--   * an asset in **ws2/fix-second** (project theta): every workspace-scoped
--     lookup in routes 14/18/19/24/25/28 must miss it, and the duplicate
--     route must 404 a source from another workspace;
--   * **two live rows with the same `asset` key**: `FileAsset.objects.get()`
--     (routes 14 and 16) raises `MultipleObjectsReturned` → 500, which no
--     other row can produce;
--   * a **never-uploaded** row (route 19's `if not asset.is_uploaded` raises
--     `attachment.activity.created` before the save);
--   * a **NULL `storage_metadata`** row and a **non-empty** one: route 19's
--     `if not asset.storage_metadata` enqueues `get_asset_object_metadata`
--     for NULL and `{}` and must not for a populated object;
--   * an asset **created by the guest** (route 19 PATCH/DELETE by a
--     restricted guest: `can_modify_asset`'s uploader branch needs a role,
--     and a GUEST role then fails the entity-type check);
--   * a **PROJECT_COVER whose project is in another workspace** (ws asset,
--     theta project): route 28's destination check is
--     `Project.objects.filter(id=…, workspace=…)` and must 404, not 403.
--
-- Ids are fixed and documented here; the scenarios' `vars` repeat them.
--
--   workspace fix-workspace  a91c3d80-5ed0-40f3-92a8-a28c015f4388
--   workspace fix-second     7045c0d6-5260-494d-a205-4cfbaab24fea
--   project alpha            b0063033-54f9-49d8-b5f3-6b7a1551c6a6
--   project theta (ws2)      08e2fc88-18a5-4758-87ec-5437bc0cf4bb
--   user admin               d44ae570-3de4-4413-af08-f686bf5b7235
--   user member              78100240-9cfd-4782-8627-25ab039c6e9a
--   user guest               31a752d6-2f12-4ac9-b23f-ae260a96c148
--   user nonmember (ws2)     e15c61cc-f495-4e69-a88f-9c2cf5cdca7b
--   issue ALPHA-010          d432015f-5a0f-4904-a2eb-d8bfea948a29
--   page Alpha Project Page  a0eb70c7-531a-4fb1-bd48-6c746066c310
--   draft "bulk edit labels" 568f87ef-9cd2-4104-b1d5-2b4e4e8ad4c4

-- ---------------------------------------------------------------------------
-- Soft-deleted (restore, and the v1/v2 manager lookups)
-- ---------------------------------------------------------------------------
INSERT INTO file_assets
  (created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
   workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
   external_id, external_source, is_uploaded, issue_id, page_id, project_id,
   size, storage_metadata, user_id, draft_issue_id, entity_identifier)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'e0a00000-0000-4000-8000-000000000001',
   '{"name": "old-spec.pdf", "type": "application/pdf", "size": 1024}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000001-old-spec.pdf',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', TRUE, '2026-01-02 00:00:00+00',
   FALSE, NULL, 'ISSUE_ATTACHMENT', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 1024, '{}', NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- USER_COVER for `member`; `users.cover_image_asset` is pointed at it below.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'e0a00000-0000-4000-8000-000000000002',
   '{"name": "cover.png", "type": "image/png", "size": 4096}',
   'user-00000000000000000000000000000002-cover.png',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   NULL, FALSE, NULL,
   FALSE, NULL, 'USER_COVER', NULL, NULL, TRUE,
   NULL, NULL, NULL, 4096, '{}',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a'),

-- ---------------------------------------------------------------------------
-- PAGE_DESCRIPTION (route 26's page branch; the fixture has no page asset)
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'e0a00000-0000-4000-8000-000000000003',
   '{"name": "page-diagram.png", "type": "image/png", "size": 2048}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000003-page-diagram.png',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'PAGE_DESCRIPTION', NULL, NULL, TRUE,
   NULL, 'a0eb70c7-531a-4fb1-bd48-6c746066c310',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 2048, '{}', NULL, NULL,
   'a0eb70c7-531a-4fb1-bd48-6c746066c310'),

-- ---------------------------------------------------------------------------
-- DRAFT_ISSUE_DESCRIPTION (route 26's draft branch)
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'e0a00000-0000-4000-8000-000000000004',
   '{"name": "draft-note.txt", "type": "text/plain", "size": 512}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000004-draft-note.txt',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'DRAFT_ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   NULL, NULL, 'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 512, '{}', NULL,
   '568f87ef-9cd2-4104-b1d5-2b4e4e8ad4c4',
   '568f87ef-9cd2-4104-b1d5-2b4e4e8ad4c4'),

-- ---------------------------------------------------------------------------
-- An asset in another workspace (ws2 / project theta), owned by ws2's admin.
-- Every `workspace__slug` / `workspace_id` lookup scoped to fix-workspace must
-- miss it; route 28's destination check sees a foreign project.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'e0a00000-0000-4000-8000-000000000005',
   '{"name": "theta-diagram.png", "type": "image/png", "size": 3072}',
   '7045c0d6-5260-494d-a205-4cfbaab24fea/00000000000000000000000000000005-theta-diagram.png',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL,
   '7045c0d6-5260-494d-a205-4cfbaab24fea', FALSE, NULL,
   FALSE, NULL, 'ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   NULL, NULL, '08e2fc88-18a5-4758-87ec-5437bc0cf4bb', 3072, '{}', NULL,
   NULL, '08e2fc88-18a5-4758-87ec-5437bc0cf4bb'),

-- ---------------------------------------------------------------------------
-- Two live rows with the same `asset` key, both created by admin:
-- `FileAsset.objects.get(asset=…, created_by=admin)` (route 16) raises
-- `MultipleObjectsReturned` → 500, as does route 14's id-only `.get()`.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'e0a00000-0000-4000-8000-000000000006',
   '{"name": "duplicate.txt", "type": "text/plain", "size": 128}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000006-duplicate.txt',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 128, '{}', NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   'e0a00000-0000-4000-8000-000000000007',
   '{"name": "duplicate.txt", "type": "text/plain", "size": 128}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000006-duplicate.txt',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 128, '{}', NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- Never uploaded, storage_metadata `{}`: route 19 confirms it (is_uploaded
-- becomes TRUE) and `{}` is falsy, so the metadata task is enqueued too.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'e0a00000-0000-4000-8000-000000000008',
   '{"name": "pending.png", "type": "image/png", "size": 8192}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000008-pending.png',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_ATTACHMENT', NULL, NULL, FALSE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 8192, '{}', NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- Already uploaded, NULL storage_metadata: route 19 skips the activity message
-- and enqueues the metadata task (`None` is falsy).
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:09+00', '2026-01-01 00:00:09+00',
   'e0a00000-0000-4000-8000-000000000009',
   '{"name": "no-metadata.png", "type": "image/png", "size": 4096}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000009-no-metadata.png',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_ATTACHMENT', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 4096, NULL, NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- Uploaded with a populated storage_metadata: route 19 must enqueue nothing.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:10+00', '2026-01-01 00:00:10+00',
   'e0a00000-0000-4000-8000-00000000000a',
   '{"name": "with-metadata.png", "type": "image/png", "size": 2048}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000a-with-metadata.png',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_ATTACHMENT', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 2048,
   '{"ContentType": "image/png", "ContentLength": 2048, "ETag": "\"00000000000000000000000000000000\"", "Metadata": {}}',
   NULL, NULL, 'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- Created by the restricted guest: route 19 PATCH by the guest passes the
-- uploader branch, then the ISSUE_ATTACHMENT rule denies a GUEST project role.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:11+00', '2026-01-01 00:00:11+00',
   'e0a00000-0000-4000-8000-00000000000b',
   '{"name": "guest-note.txt", "type": "text/plain", "size": 256}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000b-guest-note.txt',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', 256, '{}', NULL, NULL,
   'd432015f-5a0f-4904-a2eb-d8bfea948a29'),

-- ---------------------------------------------------------------------------
-- PROJECT_COVER in fix-workspace whose `project_id` is theta (fix-second):
-- route 28 with `entity_id=theta` must 404 ("Project not found"), not 403.
-- ---------------------------------------------------------------------------
  ('2026-01-01 00:00:12+00', '2026-01-01 00:00:12+00',
   'e0a00000-0000-4000-8000-00000000000c',
   '{"name": "foreign-cover.png", "type": "image/png", "size": 6144}',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388/00000000000000000000000000000c-foreign-cover.png',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', FALSE, NULL,
   FALSE, NULL, 'PROJECT_COVER', NULL, NULL, TRUE,
   NULL, NULL, '08e2fc88-18a5-4758-87ec-5437bc0cf4bb', 6144, '{}', NULL,
   NULL, '08e2fc88-18a5-4758-87ec-5437bc0cf4bb');

-- ---------------------------------------------------------------------------
-- `member`'s cover points at the new USER_COVER, so the user entity helper's
-- "delete the previous cover" branch (`v2.py:107-108`) has a previous row.
-- `User.save()` is not involved: this is a column UPDATE, and the row's
-- `token_updated_at`/`token` stay exactly as the fixture built them.
-- ---------------------------------------------------------------------------
UPDATE users
SET cover_image_asset_id = 'e0a00000-0000-4000-8000-000000000002'
WHERE id = '78100240-9cfd-4782-8627-25ab039c6e9a';
