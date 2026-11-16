-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Page-core write scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists:
--
--   * no route can create an `is_locked` page, a page owned by a different
--     user, an archived page, or a page whose parent is archived — and every
--     one of those is needed for a branch of PATCH, DELETE, archive or
--     unarchive;
--   * the archive CTE must reach a **soft-deleted** descendant and an archived
--     ancestor whose child is being unarchived, and destroy must show that its
--     children detach only reaches children linked to the URL project;
--   * archive soft-deletes every user's page favourite and destroy hard-deletes
--     every user's **live** page recent visit; the rich fixture's own page rows
--     have `project_id NULL` (the recent visit) or belong to another project
--     (`favorite_page_with_project`), which neither filter matches;
--   * `labels` accepts any live label — a label from another workspace proves
--     the gate is `Label.objects.all()` and not the page's workspace;
--   * the create's `Project.objects.get(pk=…)` is unscoped by the URL
--     workspace, which is only observable when the permission check passes:
--     the synthetic `project_members` row below makes that possible.
--
-- Ids are fixed and documented here; the scenario's `vars` repeat them.
--
--   workspace          a91c3d80-5ed0-40f3-92a8-a28c015f4388  (fix-workspace)
--   admin              d44ae570-3de4-4413-af08-f686bf5b7235
--   member             78100240-9cfd-4782-8627-25ab039c6e9a
--   project alpha      b0063033-54f9-49d8-b5f3-6b7a1551c6a6
--   project beta       ca8f652c-e89f-4c43-b91d-c2c6f9387509
--   ws fix-second      7045c0d6-5260-494d-a205-4cfbaab24fea
--   project theta      08e2fc88-18a5-4758-87ec-5437bc0cf4bb
--   ws fix-nologo      527bb2cb-4117-4d30-9e38-69a765e4f4d3
--   label design_system 3be976f6-630e-4a7e-829c-66cb027c2696
--   label cross_cutting 6a68d88a-c0eb-4f42-9cab-84c915488824
--   label alpha bug     51298d1d-71da-4345-baa7-3c68b16b1f00

-- ---------------------------------------------------------------------------
-- Pages
-- ---------------------------------------------------------------------------
INSERT INTO pages
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, description_json, description_binary, description_html,
   description_stripped, owned_by_id, access, color, parent_id, archived_at,
   is_locked, view_props, logo_props, is_global, moved_to_page,
   moved_to_project, sort_order, external_id, external_source)
VALUES
  -- Archive root: the archive CTE writes archived_at on this page, its child
  -- and its (soft-deleted) grandchild, and nowhere else.
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archive Root', '{}', NULL,
   '<p>archive root</p>', 'archive root',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1000, NULL, NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archive Child', '{}', NULL,
   '<p>archive child</p>', 'archive child',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', 
   'aa000000-0000-4000-8000-000000000001', NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1001, NULL, NULL),
  -- Soft-deleted: the CTE ignores `deleted_at`, so its archived_at moves too.
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, '2026-01-02 00:00:00+00',
   'aa000000-0000-4000-8000-000000000003',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archive Grandchild', '{}', NULL,
   '<p>archive grandchild</p>', 'archive grandchild',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', 
   'aa000000-0000-4000-8000-000000000002', NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1002, NULL, NULL),

  -- Unarchive: an archived parent, so unarchiving its child detaches it with
  -- `save(update_fields=["parent"])` (no updated_at bump), then NULLs the
  -- subtree.
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000020',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archived Parent', '{}', NULL,
   '<p>archived parent</p>', 'archived parent',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL,
   '2026-01-01', FALSE, '{"full_width": false}', '{}', FALSE, NULL, NULL,
   1003, NULL, NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000021',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Unarchive Child', '{}', NULL,
   '<p>unarchive child</p>', 'unarchive child',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000020', '2026-01-02', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1004, NULL, NULL),
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000022',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Unarchive Grandchild', '{}',
   NULL, '<p>unarchive grandchild</p>', 'unarchive grandchild',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000021', '2026-01-03', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1005, NULL, NULL),
  -- A live parent: unarchiving its child must NOT detach it.
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000024',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Live Parent', '{}', NULL,
   '<p>live parent</p>', 'live parent',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1006, NULL, NULL),
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000023',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Unarchive Live Child', '{}',
   NULL, '<p>unarchive live child</p>', 'unarchive live child',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000024', '2026-01-04', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1007, NULL, NULL),

  -- PATCH branches.
  ('2026-01-01 00:00:09+00', '2026-01-01 00:00:09+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000030',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Locked', '{}', NULL,
   '<p>locked</p>', 'locked',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, TRUE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1008, NULL, NULL),
  -- Public, owned by the member: the admin may PATCH it (public page, admin
  -- role) but changing `access` hits the handler's owner guard.
  ('2026-01-01 00:00:10+00', '2026-01-01 00:00:10+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000031',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Member Public', '{}', NULL,
   '<p>member public</p>', 'member public',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1009, NULL, NULL),
  -- Private and owned by the member: `ProjectPagePermission` denies the admin
  -- before the view runs.
  ('2026-01-01 00:00:11+00', '2026-01-01 00:00:11+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000032',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Member Private', '{}', NULL,
   '<p>member private</p>', 'member private',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 1, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1010, NULL, NULL),
  -- Two live label rows and one soft-deleted one: PATCH `labels` H04s only the
  -- live rows.
  ('2026-01-01 00:00:12+00', '2026-01-01 00:00:12+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000033',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Label Replace', '{}', NULL,
   '<p>label replace</p>', 'label replace',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1011, NULL, NULL),
  -- Linked to alpha and beta.
  ('2026-01-01 00:00:13+00', '2026-01-01 00:00:13+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000060',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Two Links', '{}', NULL,
   '<p>two links</p>', 'two links',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1012, NULL, NULL),

  -- Destroy: an archived parent with one child linked to alpha (detached) and
  -- one linked to beta only (kept).
  ('2026-01-01 00:00:14+00', '2026-01-01 00:00:14+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000040',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Parent', '{}', NULL,
   '<p>destroy parent</p>', 'destroy parent',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, '2026-01-01', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1013, NULL, NULL),
  ('2026-01-01 00:00:15+00', '2026-01-01 00:00:15+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000041',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Alpha Child', '{}',
   NULL, '<p>destroy alpha child</p>', 'destroy alpha child',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000040', NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1014, NULL, NULL),
  ('2026-01-01 00:00:16+00', '2026-01-01 00:00:16+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000042',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Beta Child', '{}',
   NULL, '<p>destroy beta child</p>', 'destroy beta child',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000040', NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1015, NULL, NULL),
  -- Owned by the member: a member owner passes `ProjectPagePermission` for
  -- DELETE even though the method table allows admins only.
  ('2026-01-01 00:00:17+00', '2026-01-01 00:00:17+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000050',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Member Owned', '{}',
   NULL, '<p>destroy member owned</p>', 'destroy member owned',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '', NULL, '2026-01-01', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1016, NULL, NULL),
  -- Owned by the member; the admin deletes it through the role-20 probe.
  ('2026-01-01 00:00:18+00', '2026-01-01 00:00:18+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000051',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Admin Role', '{}',
   NULL, '<p>destroy admin role</p>', 'destroy admin role',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '', NULL, '2026-01-01', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1017, NULL, NULL),
  -- Owned by the admin: the member (role 15) is neither owner nor admin.
  ('2026-01-01 00:00:19+00', '2026-01-01 00:00:19+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000052',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Denied', '{}', NULL,
   '<p>destroy denied</p>', 'destroy denied',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, '2026-01-01', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1018, NULL, NULL),
  ('2026-01-01 00:00:20+00', '2026-01-01 00:00:20+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000053',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Destroy Unarchived', '{}',
   NULL, '<p>destroy unarchived</p>', 'destroy unarchived',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1019, NULL, NULL),
  -- Archived but linked to beta only: DELETE through alpha misses the lookup.
  ('2026-01-01 00:00:21+00', '2026-01-01 00:00:21+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000054',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Beta Only', '{}', NULL,
   '<p>beta only</p>', 'beta only',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, '2026-01-01', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1020, NULL, NULL),

  -- Archive guard branches.
  ('2026-01-01 00:00:22+00', '2026-01-01 00:00:22+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000070',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archive NonOwner', '{}', NULL,
   '<p>archive nonowner</p>', 'archive nonowner',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1021, NULL, NULL),
  ('2026-01-01 00:00:23+00', '2026-01-01 00:00:23+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000071',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Archive Admin Ok', '{}', NULL,
   '<p>archive admin ok</p>', 'archive admin ok',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1022, NULL, NULL),
  ('2026-01-01 00:00:24+00', '2026-01-01 00:00:24+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000072',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Unarchive NonOwner', '{}',
   NULL, '<p>unarchive nonowner</p>', 'unarchive nonowner',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, '2026-01-02', FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1023, NULL, NULL),

  -- PATCH: one page per branch that changes state.
  ('2026-01-01 00:00:25+00', '2026-01-01 00:00:25+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000080',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Patch', '{}', NULL,
   '<p>patch</p>', 'patch',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1024, NULL, NULL),
  ('2026-01-01 00:00:26+00', '2026-01-01 00:00:26+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000081',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Patch Html', '{}', NULL,
   '<p>old body</p>', 'old body',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1025, NULL, NULL),
  ('2026-01-01 00:00:27+00', '2026-01-01 00:00:27+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000082',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Patch Created By', '{}', NULL,
   '<p>patch created by</p>', 'patch created by',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1026, NULL, NULL),
  ('2026-01-01 00:00:28+00', '2026-01-01 00:00:28+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000083',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Patch Parent', '{}', NULL,
   '<p>patch parent</p>', 'patch parent',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '',
   'aa000000-0000-4000-8000-000000000024', NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1027, NULL, NULL),
  ('2026-01-01 00:00:29+00', '2026-01-01 00:00:29+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'aa000000-0000-4000-8000-000000000084',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Seed Patch Lists', '{}', NULL,
   '<p>patch lists</p>', 'patch lists',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 0, '', NULL, NULL, FALSE,
   '{"full_width": false}', '{}', FALSE, NULL, NULL, 1028, NULL, NULL);

-- ---------------------------------------------------------------------------
-- Project links
-- ---------------------------------------------------------------------------
INSERT INTO project_pages
  (created_at, updated_at, id, page_id, project_id, created_by_id,
   updated_by_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'ab000000-0000-4000-8000-000000000001',
   'aa000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'ab000000-0000-4000-8000-000000000002',
   'aa000000-0000-4000-8000-000000000002',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'ab000000-0000-4000-8000-000000000003',
   'aa000000-0000-4000-8000-000000000003',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'ab000000-0000-4000-8000-000000000004',
   'aa000000-0000-4000-8000-000000000020',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'ab000000-0000-4000-8000-000000000005',
   'aa000000-0000-4000-8000-000000000021',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'ab000000-0000-4000-8000-000000000006',
   'aa000000-0000-4000-8000-000000000022',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   'ab000000-0000-4000-8000-000000000007',
   'aa000000-0000-4000-8000-000000000024',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'ab000000-0000-4000-8000-000000000008',
   'aa000000-0000-4000-8000-000000000023',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:09+00', '2026-01-01 00:00:09+00',
   'ab000000-0000-4000-8000-000000000009',
   'aa000000-0000-4000-8000-000000000030',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:10+00', '2026-01-01 00:00:10+00',
   'ab000000-0000-4000-8000-000000000010',
   'aa000000-0000-4000-8000-000000000031',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:11+00', '2026-01-01 00:00:11+00',
   'ab000000-0000-4000-8000-000000000011',
   'aa000000-0000-4000-8000-000000000032',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:12+00', '2026-01-01 00:00:12+00',
   'ab000000-0000-4000-8000-000000000012',
   'aa000000-0000-4000-8000-000000000033',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:13+00', '2026-01-01 00:00:13+00',
   'ab000000-0000-4000-8000-000000000013',
   'aa000000-0000-4000-8000-000000000060',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:14+00', '2026-01-01 00:00:14+00',
   'ab000000-0000-4000-8000-000000000014',
   'aa000000-0000-4000-8000-000000000060',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:15+00', '2026-01-01 00:00:15+00',
   'ab000000-0000-4000-8000-000000000015',
   'aa000000-0000-4000-8000-000000000040',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:16+00', '2026-01-01 00:00:16+00',
   'ab000000-0000-4000-8000-000000000016',
   'aa000000-0000-4000-8000-000000000040',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:17+00', '2026-01-01 00:00:17+00',
   'ab000000-0000-4000-8000-000000000017',
   'aa000000-0000-4000-8000-000000000041',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:18+00', '2026-01-01 00:00:18+00',
   'ab000000-0000-4000-8000-000000000018',
   'aa000000-0000-4000-8000-000000000042',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:19+00', '2026-01-01 00:00:19+00',
   'ab000000-0000-4000-8000-000000000019',
   'aa000000-0000-4000-8000-000000000050',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:20+00', '2026-01-01 00:00:20+00',
   'ab000000-0000-4000-8000-000000000020',
   'aa000000-0000-4000-8000-000000000051',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:21+00', '2026-01-01 00:00:21+00',
   'ab000000-0000-4000-8000-000000000021',
   'aa000000-0000-4000-8000-000000000052',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:22+00', '2026-01-01 00:00:22+00',
   'ab000000-0000-4000-8000-000000000022',
   'aa000000-0000-4000-8000-000000000053',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:23+00', '2026-01-01 00:00:23+00',
   'ab000000-0000-4000-8000-000000000023',
   'aa000000-0000-4000-8000-000000000054',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:24+00', '2026-01-01 00:00:24+00',
   'ab000000-0000-4000-8000-000000000024',
   'aa000000-0000-4000-8000-000000000070',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:25+00', '2026-01-01 00:00:25+00',
   'ab000000-0000-4000-8000-000000000025',
   'aa000000-0000-4000-8000-000000000071',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:26+00', '2026-01-01 00:00:26+00',
   'ab000000-0000-4000-8000-000000000026',
   'aa000000-0000-4000-8000-000000000072',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:27+00', '2026-01-01 00:00:27+00',
   'ab000000-0000-4000-8000-000000000027',
   'aa000000-0000-4000-8000-000000000080',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:28+00', '2026-01-01 00:00:28+00',
   'ab000000-0000-4000-8000-000000000028',
   'aa000000-0000-4000-8000-000000000081',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:29+00', '2026-01-01 00:00:29+00',
   'ab000000-0000-4000-8000-000000000029',
   'aa000000-0000-4000-8000-000000000082',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:30+00', '2026-01-01 00:00:30+00',
   'ab000000-0000-4000-8000-000000000030',
   'aa000000-0000-4000-8000-000000000083',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:31+00', '2026-01-01 00:00:31+00',
   'ab000000-0000-4000-8000-000000000031',
   'aa000000-0000-4000-8000-000000000084',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

-- ---------------------------------------------------------------------------
-- Page labels: two live rows and one already soft-deleted, all on
-- `page_seed_labels`. PATCH `labels` must H04 only the two live ones.
-- ---------------------------------------------------------------------------
INSERT INTO page_labels
  (created_at, updated_at, id, label_id, page_id, created_by_id, updated_by_id,
   workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'ac000000-0000-4000-8000-000000000001',
   '3be976f6-630e-4a7e-829c-66cb027c2696',
   'aa000000-0000-4000-8000-000000000033',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'ac000000-0000-4000-8000-000000000002',
   '51298d1d-71da-4345-baa7-3c68b16b1f00',
   'aa000000-0000-4000-8000-000000000033',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:03+00', '2026-01-02 00:00:00+00',
   'ac000000-0000-4000-8000-000000000003',
   '6a68d88a-c0eb-4f42-9cab-84c915488824',
   'aa000000-0000-4000-8000-000000000033',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '2026-01-02 00:00:00+00');

-- ---------------------------------------------------------------------------
-- A label in another workspace: `PageSerializer.labels` accepts any live
-- `Label` (`plane/app/serializers/page.py:28`).
-- ---------------------------------------------------------------------------
INSERT INTO labels
  (created_at, updated_at, id, name, description, created_by_id, project_id,
   updated_by_id, workspace_id, parent_id, color, sort_order, external_id,
   external_source, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'bb000000-0000-4000-8000-000000000001', 'Foreign Label',
   'label from another workspace', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   NULL, NULL, '527bb2cb-4117-4d30-9e38-69a765e4f4d3', NULL, '#123456', 100,
   NULL, NULL, NULL);

-- ---------------------------------------------------------------------------
-- Favourites: archive soft-deletes **every** page favourite of its project;
-- destroy does the same. Rows with another entity_type or another project must
-- survive, which is what proves the predicates.
-- ---------------------------------------------------------------------------
INSERT INTO user_favorites
  (created_at, updated_at, id, entity_type, entity_identifier, name, is_folder,
   sequence, created_by_id, parent_id, project_id, updated_by_id, user_id,
   workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000001', 'page',
   'aa000000-0000-4000-8000-000000000001', NULL, FALSE, 2000.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000002', 'page',
   'aa000000-0000-4000-8000-000000000001', NULL, FALSE, 2001.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Wrong entity_type: kept.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000003', 'view',
   'aa000000-0000-4000-8000-000000000001', NULL, FALSE, 2002.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Destroy: both users' favourites are H04-ed.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000004', 'page',
   'aa000000-0000-4000-8000-000000000050', NULL, FALSE, 2003.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000005', 'page',
   'aa000000-0000-4000-8000-000000000050', NULL, FALSE, 2004.0,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Another project, another user: kept (`user_favorites` has a partial
  -- unique index on (entity_type, entity_identifier, user), so this row needs a
  -- user who has no other favourite on the page).
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ad000000-0000-4000-8000-000000000006', 'page',
   'aa000000-0000-4000-8000-000000000050', NULL, FALSE, 2005.0,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   '31a752d6-2f12-4ac9-b23f-ae260a96c148',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

-- ---------------------------------------------------------------------------
-- Recent visits: destroy hard-deletes the **live** rows only; the soft-deleted
-- row and the rows of another entity_type / project must survive.
-- ---------------------------------------------------------------------------
INSERT INTO user_recent_visits
  (created_at, updated_at, id, entity_identifier, entity_name, visited_at,
   created_by_id, project_id, updated_by_id, user_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ae000000-0000-4000-8000-000000000001',
   'aa000000-0000-4000-8000-000000000050', 'page',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ae000000-0000-4000-8000-000000000002',
   'aa000000-0000-4000-8000-000000000050', 'page',
   '2026-01-01 00:00:00+00', '78100240-9cfd-4782-8627-25ab039c6e9a',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Already soft-deleted: the manager predicate keeps it out of the DELETE.
  ('2026-01-01 00:00:00+00', '2026-01-02 00:00:00+00',
   'ae000000-0000-4000-8000-000000000003',
   'aa000000-0000-4000-8000-000000000050', 'page',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', '2026-01-02 00:00:00+00'),
  -- Another entity_name: kept.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ae000000-0000-4000-8000-000000000004',
   'aa000000-0000-4000-8000-000000000050', 'view',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL),
  -- Another project: kept.
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'ae000000-0000-4000-8000-000000000005',
   'aa000000-0000-4000-8000-000000000050', 'page',
   '2026-01-01 00:00:00+00', 'd44ae570-3de4-4413-af08-f686bf5b7235',
   'ca8f652c-e89f-4c43-b91d-c2c6f9387509', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

-- ---------------------------------------------------------------------------
-- The synthetic membership that lets the create's unscoped
-- `Project.objects.get(pk=…)` be observed: the permission check filters the
-- member's **workspace** by the URL slug, so a row whose workspace is
-- `fix-workspace` but whose project lives in `fix-second` passes it. The page
-- the create writes lands in `fix-second` and the re-fetch — scoped to
-- `fix-workspace` — misses it.
-- ---------------------------------------------------------------------------
INSERT INTO project_members
  (created_at, updated_at, id, project_id, workspace_id, member_id, comment,
   role, view_props, default_props, preferences, sort_order, is_active,
   created_by_id, updated_by_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'cc000000-0000-4000-8000-000000000001',
   '08e2fc88-18a5-4758-87ec-5437bc0cf4bb',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd44ae570-3de4-4413-af08-f686bf5b7235', '', 20,
   '{}', '{}', '{}', 100, TRUE,
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL);
