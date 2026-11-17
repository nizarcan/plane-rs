-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Set A fixture (`plans/USERS_ME.md` §3, `users-me-profile.yaml`).
--
--   1. Four live `file_assets` rows and one soft-deleted one, so 162 PATCH's
--      `validate_avatar_asset`/`validate_cover_image_asset`
--      (`plane/app/serializers/user.py:26-38`) has an own asset of each entity
--      type, another account's asset, and the wrong entity type. The generated
--      `PrimaryKeyRelatedField` lookup already hides the soft-deleted row, so
--      that step is a `does_not_exist` 400 instead of `Invalid asset.`.
--   2. `no_workspace` loses its profile: 167/171/172's
--      `Profile.objects.get(...)` is then the family 404
--      `{"error": "The required object does not exist."}`, and the body is
--      never parsed. Every other fixture user keeps its profile.
--
-- Ids are `d1000000-…`; timestamps are fixed so the rows are identical on both
-- sides and outside the harness's "now" window.

INSERT INTO file_assets
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   attributes, asset, user_id, workspace_id, draft_issue_id, project_id,
   issue_id, comment_id, page_id, entity_type, entity_identifier, is_deleted,
   is_archived, external_id, external_source, size, is_uploaded,
   storage_metadata)
VALUES
  -- 1. member's own avatar: linked by 162 PATCH.
  ('2026-01-01 00:01:01+00', '2026-01-01 00:01:01+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'd1000000-0000-4000-8000-000000000001', '{}',
   'fixture-avatars/d1000000-member.png',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL, NULL, NULL, NULL,
   'USER_AVATAR', NULL, FALSE, FALSE, NULL, NULL, 0, FALSE, '{}'),
  -- 2. member's own cover: linked by 162 PATCH.
  ('2026-01-01 00:01:02+00', '2026-01-01 00:01:02+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'd1000000-0000-4000-8000-000000000002', '{}',
   'fixture-covers/d1000000-member.png',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL, NULL, NULL, NULL,
   'USER_COVER', NULL, FALSE, FALSE, NULL, NULL, 0, FALSE, '{}'),
  -- 3. guest's avatar: the ownership check refuses it for member.
  ('2026-01-01 00:01:03+00', '2026-01-01 00:01:03+00',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL,
   'd1000000-0000-4000-8000-000000000003', '{}',
   'fixture-avatars/d1000000-guest.png',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', NULL, NULL, NULL, NULL, NULL, NULL,
   'USER_AVATAR', NULL, FALSE, FALSE, NULL, NULL, 0, FALSE, '{}'),
  -- 4. member's own row with the wrong entity type: `Invalid asset.`.
  ('2026-01-01 00:01:04+00', '2026-01-01 00:01:04+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'd1000000-0000-4000-8000-000000000004', '{}',
   'fixture-logos/d1000000-member.png',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL, NULL, NULL, NULL,
   'WORKSPACE_LOGO', NULL, FALSE, FALSE, NULL, NULL, 0, FALSE, '{}'),
  -- 5. member's own soft-deleted avatar: the field's queryset hides it.
  ('2026-01-01 00:01:05+00', '2026-01-01 00:01:05+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   '2025-12-01 00:00:00+00',
   'd1000000-0000-4000-8000-000000000005', '{}',
   'fixture-avatars/d1000000-deleted.png',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL, NULL, NULL, NULL,
   'USER_AVATAR', NULL, FALSE, FALSE, NULL, NULL, 0, FALSE, '{}');

-- A user whose profile row is gone: the three profile routes' 404.
DELETE FROM profiles WHERE user_id = 'a69a1178-503e-4e62-a07b-f89c6af07687';
