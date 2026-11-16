-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Page actions (lock/unlock/access/description) fixture. Applied to both
-- differential targets before the baseline snapshot (`vars: { fixture_sql: … }`),
-- so these rows are not step diffs until a request changes them.
--
-- Why it exists:
--
--   * the rich fixture's `page_locked` and `page_archived` have no
--     `project_pages` link, so `ProjectPagePermission` answers 403 before the
--     description route can answer its 400 `PAGE_LOCKED`/`PAGE_ARCHIVED` body;
--   * no page is owned by the member *and* linked to a project, so the
--     non-owner `400 "Access cannot be updated since this page is owned by
--     someone else"` branch is unreachable;
--   * `Page.save()` (H26) is only observable when the stored
--     `description_stripped` disagrees with `description_html`, which no route
--     can produce;
--   * a soft-deleted `project_pages` link is needed for the permission 403 the
--     handler never sees.
--
-- Ids are `b9…` so they cannot collide with the builder's stable ids.

-- Links the locked, archived and member-owned private pages into alpha.
INSERT INTO project_pages
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, page_id, workspace_id)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   '2f4ab7d5-1db8-4b1b-9305-05e6985e02c4',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- archived page (`archived_at` is set by the builder)
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'f73087a0-0b3f-4bb0-a8ba-5293670c42fa',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- private page owned by the *member*: the member passes, the admin is 403.
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   '1d0ba1df-af27-4705-8ead-5b4c4ceceeaf',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- member-owned public page (the non-owner access 400 and the permissive
  -- description 200 for a non-owner).
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000004',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'b9000000-0000-4000-8000-000000000010',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- soft-deleted link to alpha: `ProjectPagePermission` answers 403.
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, '2026-02-01 00:00:00+00',
   'b9000000-0000-4000-8000-000000000005',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'b9000000-0000-4000-8000-000000000011',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- the member-owned public page's link is stored in project_pages above;
  -- this page (owned by the admin) is the H26 probe.
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000006',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'b9000000-0000-4000-8000-000000000012',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388');

INSERT INTO pages
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description_json, description_binary,
   description_html, description_stripped, owned_by_id, access, color,
   parent_id, archived_at, is_locked, view_props, logo_props, is_global,
   moved_to_page, moved_to_project, sort_order, external_id, external_source)
VALUES
  -- owned by the member, public: the non-owner admin gets the 400 on `access`
  -- and 200 on `description`.
  ('2026-01-02 00:00:01+00', '2026-01-02 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000010',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Member Owned Public',
   '{"type": "doc", "content": []}', '\x6d656d626572',
   '<p>member owned body</p>', 'member owned body',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '#F59E0B',
   NULL, NULL, FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL,
   65535, NULL, NULL),
  -- linked to alpha by a soft-deleted row: permission denies every action.
  ('2026-01-02 00:00:02+00', '2026-01-02 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000011',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Detached Page',
   '{}', NULL, '<p>detached</p>', 'detached',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '#F59E0B',
   NULL, NULL, FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL,
   65535, NULL, NULL),
  -- H26 probe: `description_stripped` is stale until the next full save.
  ('2026-01-02 00:00:03+00', '2026-01-02 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'b9000000-0000-4000-8000-000000000012',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Stale Stripped',
   '{"type": "doc"}', NULL, '<p>alpha &amp; beta</p>', 'STALE',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '#F59E0B',
   NULL, NULL, FALSE, '{"full_width": true}', '{"in_use": "emoji"}', FALSE,
   NULL, NULL, 65535, 'ext-1', 'fixture');
