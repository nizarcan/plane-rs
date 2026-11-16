-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Page-duplicate write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request copies them.
--
-- Why it exists:
--
--   * the rich fixture has no page linked to project alpha that is locked,
--     archived, has a parent or a private owner other than the admin, and
--     those are exactly the columns `page.save()` copies;
--   * the duplicate's two task arguments need a page whose `project_pages`
--     order is deterministic: link `created_at` values are distinct, so the
--     `copy_s3_objects` `project_id` (the last link under `-created_at`) is
--     the *other* project, proving it is the loop variable and not the URL
--     project;
--   * one page has a soft-deleted second link, which the `values_list`
--     queryset's default manager must skip;
--   * one page has `description_html = ''` with a stale `description_stripped`,
--     which H26 must rewrite to NULL on the copy;
--   * the source rows carry a non-null `description_binary`, a stale
--     `description_stripped`, `is_global`, `external_id`/`external_source`,
--     `view_props`/`logo_props` and a fractional `sort_order`, so a missed
--     column copy shows up in the database diff.
--
-- Ids are the scenario's `page_*`/`link_*` variables. `parent_id` points at the
-- rich fixture's "Engineering Home" page.

INSERT INTO pages
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description_json, description_binary, description_html,
   description_stripped, owned_by_id, access, color, parent_id, archived_at,
   is_locked, view_props, logo_props, is_global, moved_to_page,
   moved_to_project, sort_order, external_id, external_source)
VALUES
  -- Everything a copy must carry; the caller is the admin.
  ('2026-05-01 00:00:01+00', '2026-05-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'a1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Full Source',
   '{"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "hello"}]}]}',
   '\x00ff10'::bytea,
   '<p>Duplicate &amp; full&#39;s body</p>',
   'STALE-STRIPPED',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '#ABCDEF', NULL, NULL,
   FALSE, '{"full_width": true, "x": [1, 2]}',
   '{"in_use": "emoji", "emoji": {"value": "🧪"}}', TRUE,
   'a0000000-0000-4000-8000-00000000aa01',
   'a0000000-0000-4000-8000-00000000bb01',
   12345.5, 'ext-dup-1', 'harness'),
  -- is_locked is copied though no route writes it.
  ('2026-05-01 00:00:02+00', '2026-05-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Locked', '{}', NULL, '<p>locked body</p>', 'locked body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   TRUE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- archived_at is a date and copied as-is.
  ('2026-05-01 00:00:03+00', '2026-05-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000003',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Archived', '{}', NULL, '<p>archived body</p>', 'archived body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, '2026-03-04',
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- Owned by the admin but copied by the member, so the copy's
  -- owned_by/created_by/updated_by must all become the member.
  ('2026-05-01 00:00:04+00', '2026-05-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000004',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Child', '{}', NULL, '<p>child body</p>', 'child body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', 
   'b3613ad7-ebd4-4cc9-aec3-a15eaa972482', NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- Private, owned by the member: the member may copy it, anyone else is
  -- refused by ProjectPagePermission before the view runs.
  ('2026-05-01 00:00:05+00', '2026-05-01 00:00:05+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000005',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Private', '{}', NULL, '<p>private body</p>', 'private body',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 1, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- Public but owned by the guest: the permission class's owner branch lets the
  -- guest through POST even though the role table would not.
  ('2026-05-01 00:00:06+00', '2026-05-01 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000006',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Guest Owned', '{}', NULL, '<p>guest body</p>', 'guest body',
   '31a752d6-2f12-4ac9-b23f-ae260a96c148', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- Two live links with distinct created_at: alpha (newest, the URL project)
  -- and beta (oldest). The duplicate inserts both, in the queryset's
  -- `-created_at` order, so the copy task's project_id is beta.
  ('2026-05-01 00:00:07+00', '2026-05-01 00:00:07+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Two Links', '{}', NULL, '<p>two links body</p>', 'two links body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- One live alpha link plus a soft-deleted beta link: only one copy.
  ('2026-05-01 00:00:08+00', '2026-05-01 00:00:08+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000008',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Soft Link', '{}', NULL, '<p>soft link body</p>', 'soft link body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- Linked to beta only: unreachable through the alpha URL.
  ('2026-05-01 00:00:09+00', '2026-05-01 00:00:09+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-000000000009',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Beta Only', '{}', NULL, '<p>beta only body</p>', 'beta only body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- `description_html = ''` is the one arm H26 turns into SQL NULL.
  ('2026-05-01 00:00:10+00', '2026-05-01 00:00:10+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-00000000000a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'Duplicate Empty Description', '{}', NULL, '', 'STALE-NULL',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL),
  -- A blank name still gains " (Copy)".
  ('2026-05-01 00:00:11+00', '2026-05-01 00:00:11+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a1000000-0000-4000-8000-00000000000b',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '', '{}', NULL, '<p>blank name</p>', 'blank name',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL,
   FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL, 65535, NULL, NULL);

INSERT INTO project_pages
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, page_id, workspace_id)
VALUES
  ('2026-05-02 00:00:01+00', '2026-05-02 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:02+00', '2026-05-02 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:03+00', '2026-05-02 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000003',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:04+00', '2026-05-02 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000004',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000004',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:05+00', '2026-05-02 00:00:05+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000005',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000005',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:06+00', '2026-05-02 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000006',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000006',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- The two-link page: alpha is newer, so it is the first loop iteration and
  -- beta the last (the value the copy task receives).
  ('2026-05-02 00:00:07+00', '2026-05-02 00:00:07+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000007',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a1000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:08+00', '2026-05-02 00:00:08+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-000000000008',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  -- The soft-deleted beta link the copy loop must skip.
  ('2026-05-02 00:00:09+00', '2026-05-02 00:00:09+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-05-03 00:00:00+00',
   'a2000000-0000-4000-8000-000000000009',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a1000000-0000-4000-8000-000000000008',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:10+00', '2026-05-02 00:00:10+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-00000000000a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-000000000008',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:11+00', '2026-05-02 00:00:11+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-00000000000b',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'a1000000-0000-4000-8000-000000000009',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:12+00', '2026-05-02 00:00:12+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-00000000000c',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-00000000000a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388'),
  ('2026-05-02 00:00:13+00', '2026-05-02 00:00:13+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'a2000000-0000-4000-8000-00000000000d',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a1000000-0000-4000-8000-00000000000b',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388');
