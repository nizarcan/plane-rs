-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Shared workspace-core write fixture (`plans/WORKSPACES_CORE.md` §4,
-- "Fixture"). Applied to both differential targets before the baseline
-- snapshot (`vars: { fixture_sql: … }`), so these rows are not step diffs until
-- a request changes them.
--
-- Why it exists — the rich fixture (`tools/fixtures/build.py`) cannot reach
-- these branches:
--
--   * `Workspace.delete` appends `__<epoch>` to the slug
--     (`plane/db/models/workspace.py:156-176`). A 40-character slug plus the
--     2 + 10 characters of the suffix is 52, over `varchar(48)`: the UPDATE
--     fails *after* the soft delete and the cascade publish, so route 187's
--     DELETE is a 500 with the row already deleted (`MODEL_HOOKS.md:506-516`).
--     `fix-workspace` is shorter, so the happy path exists too.
--   * `is_sole_admin_of_any_project` (`plane/app/views/workspace/member.py:31-
--     43`) counts **every** `project_members` row of a project in a raw LEFT
--     JOIN, with no `deleted_at`/`is_active` predicate. A project with a single
--     live admin (`mu`) makes the guard fire; a project whose second row is
--     soft-deleted (`nu`) must *not*, because the ghost still counts towards
--     `total_members`. `zeta` (admin only) covers the admin-only case the
--     existing fixture already has.
--   * route 191's activate branch needs a **membership row** for the accepting
--     user: `admin` is not a member of `fix-second`, so a deactivated row there
--     plus an invitation for the same address exercises
--     `workspace_member.save()` (`invite.py:216-222`) instead of the create
--     branch.
--   * route 191's "already responded" 400 (`invite.py:247-250`), 191's
--     case-sensitive `User.objects.filter(email=…)` miss (the pre-check is
--     case-insensitive, `invite.py:183-197` vs `:207`), and route 190's
--     soft delete of an invitation it consumed all need invitations the other
--     fixture rows cannot provide: `fix.invitee`'s invitation must survive for
--     191, so 190 gets a dedicated one.
--   * routes 199/200/218/219 need a theme and a quick link in a **second**
--     workspace, and soft-deleted variants the managers must hide.
--   * route 220's PATCH on `quick_tutorial`/`new_at_plane` needs home
--     preference rows for a non-admin member: the fixture seeds admin only, and
--     `WorkspaceHomePreferenceViewSet.get` never creates those two keys
--     (`plane/app/views/workspace/home.py:34-37`).
--   * route 207's `get_or_create` needs a second workspace's properties row
--     (and a soft-deleted row, which the partial unique index allows).
--   * route 186's response renders `Workspace.logo_url`; a `logo_asset` of a
--     non-static entity type reads the asset's own workspace, so a
--     workspace-less asset raises `AttributeError` inside the property and DRF
--     omits the key (SkipField) while the create still succeeds. No fixture
--     asset has a NULL workspace.
--
-- `project_identifiers` needs no rows here: `build.py` creates one per project,
-- so the soft-deleted `delta` project and the archived `gamma` project already
-- have one (route 138's DELETE reads them through `ProjectIdentifier.objects`,
-- which filters `deleted_at` only).
--
-- Ids are `e0000000-…` and documented inline; timestamps are fixed so the rows
-- are identical on both sides and outside the harness's "now" window.

-- 1. A 40-character slug: `slug__<epoch>` overflows `varchar(48)`.
INSERT INTO workspaces
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   name, logo, logo_asset_id, owner_id, slug, organization_size, timezone,
   background_color)
VALUES
  ('2026-01-01 00:00:01+00', '2026-01-01 00:00:01+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000001',
   'Fixture Long Slug', '', NULL,
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   'fix-long-slug-abcdefghijklmnopqrstuvwxyz', '1-10', 'UTC', '#e6e7eb');

INSERT INTO workspace_members
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, member_id, role, company_role, view_props, default_props,
   issue_props, is_active, getting_started_checklist, tips, explored_features)
VALUES
  ('2026-01-01 00:00:02+00', '2026-01-01 00:00:02+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000002',
   'e0000000-0000-4000-8000-000000000001',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 20, 'admin at fixtures',
   '{}', '{}', '{}', TRUE, '{}', '{}', '{}');

-- 2. `mu`: member2 is the only member, and an admin. Demoting member2 to guest
--    in `fix-workspace` must be refused (194 PATCH).
INSERT INTO projects
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, name,
   description, description_text, description_html, network, workspace_id,
   identifier, default_assignee_id, project_lead_id, emoji, icon_prop,
   module_view, cycle_view, issue_views_view, page_view, intake_view,
   is_time_tracking_enabled, is_issue_type_enabled, guest_view_all_features,
   cover_image, cover_image_asset_id, estimate_id, archive_in, close_in,
   logo_props, default_state_id, archived_at, timezone, external_source,
   external_id)
VALUES
  ('2026-01-01 00:00:03+00', '2026-01-01 00:00:03+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000010',
   'Mu Sole', 'Mu Sole description', '{}', '{}', 2,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'MUS', NULL, NULL, NULL, NULL,
   TRUE, TRUE, TRUE, TRUE, FALSE, FALSE, FALSE, FALSE, '', NULL, NULL, 0, 0,
   '{}', NULL, NULL, 'UTC', NULL, NULL),
  ('2026-01-01 00:00:04+00', '2026-01-01 00:00:04+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000020',
   'Nu Ghost', 'Nu Ghost description', '{}', '{}', 2,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'NUG', NULL, NULL, NULL, NULL,
   TRUE, TRUE, TRUE, TRUE, FALSE, FALSE, FALSE, FALSE, '', NULL, NULL, 0, 0,
   '{}', NULL, NULL, 'UTC', NULL, NULL);

-- `nu`'s second member is soft-deleted: `Count("project_projectmember")` still
-- counts it, so member2 is not a "sole admin" of `nu`.
INSERT INTO project_members
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, member_id, comment, role, view_props,
   default_props, preferences, sort_order, is_active)
VALUES
  ('2026-01-01 00:00:05+00', '2026-01-01 00:00:05+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000011',
   'e0000000-0000-4000-8000-000000000010',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'bc437914-c250-4c9f-ac69-85003fdf6205', NULL, 20, '{}', '{}', '{}', 65535,
   TRUE),
  ('2026-01-01 00:00:06+00', '2026-01-01 00:00:06+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000021',
   'e0000000-0000-4000-8000-000000000020',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'bc437914-c250-4c9f-ac69-85003fdf6205', NULL, 20, '{}', '{}', '{}', 65535,
   TRUE),
  ('2026-01-01 00:00:07+00', '2026-01-01 00:00:07+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:07+00',
   'e0000000-0000-4000-8000-000000000022',
   'e0000000-0000-4000-8000-000000000020',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, 15, '{}', '{}', '{}', 65535,
   TRUE);

-- 3. `admin` deactivated in `fix-second`, plus the invitation that reactivates
--    the row through 191's `workspace_member.save()` branch.
INSERT INTO workspace_members
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, member_id, role, company_role, view_props, default_props,
   issue_props, is_active, getting_started_checklist, tips, explored_features)
VALUES
  ('2026-01-01 00:00:08+00', '2026-01-01 00:00:08+00',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, NULL,
   'e0000000-0000-4000-8000-000000000030',
   '7045c0d6-5260-494d-a205-4cfbaab24fea',
   'd44ae570-3de4-4413-af08-f686bf5b7235', 15, NULL,
   '{}', '{}', '{}', FALSE, '{}', '{}', '{}');

INSERT INTO workspace_member_invites
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, email, accepted, token, message, responded_at, role)
VALUES
  -- 191 activate branch: the address matches a deactivated membership.
  ('2026-01-01 00:00:09+00', '2026-01-01 00:00:09+00',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, NULL,
   'e0000000-0000-4000-8000-000000000031',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', 'fix.admin@example.com', FALSE,
   'e000000000000000000000000000000000000000000000000000000000000031',
   NULL, NULL, 15),
  -- 191's case-sensitive `User.objects.filter(email=…)` miss: the pre-check is
  -- case-insensitive, the lookup is not, so the view answers 200 "Accepted"
  -- and leaves the invitation untouched (`invite.py:207-243`).
  ('2026-01-01 00:00:10+00', '2026-01-01 00:00:10+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000032',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Fix.MixedCase@Example.com', FALSE,
   'e000000000000000000000000000000000000000000000000000000000000032',
   NULL, NULL, 15),
  -- 191's already-responded 400 (`invite.py:247-250`).
  ('2026-01-01 00:00:11+00', '2026-01-01 00:00:11+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000033',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'fix.guest2@example.com', FALSE,
   'e000000000000000000000000000000000000000000000000000000000000033',
   'Already answered', '2026-01-01 00:00:11+00', 5),
  -- 190's own invitation: accepting it must not consume the rows above.
  ('2026-01-01 00:00:12+00', '2026-01-01 00:00:12+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e0000000-0000-4000-8000-000000000034',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'fix.lonely@example.com', FALSE,
   'e000000000000000000000000000000000000000000000000000000000000034',
   NULL, NULL, 15);

-- 4. A theme and a quick link in `fix-second`, plus soft-deleted variants in
--    `fix-workspace` that the `objects` managers must hide.
INSERT INTO workspace_themes
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, name, actor_id, colors)
VALUES
  ('2026-01-01 00:00:13+00', '2026-01-01 00:00:13+00',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, NULL,
   'e0000000-0000-4000-8000-000000000040',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', 'Second Midnight',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b',
   '{"background": "#0F172A", "accent": "#1E293B", "text": "#111827"}'),
  ('2026-01-01 00:00:14+00', '2026-01-01 00:00:14+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:14+00',
   'e0000000-0000-4000-8000-000000000041',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'Retired Theme',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   '{"background": "#111827", "accent": "#374151", "text": "#F9FAFB"}');

INSERT INTO workspace_user_links
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, title, url, metadata, owner_id)
VALUES
  ('2026-01-01 00:00:15+00', '2026-01-01 00:00:15+00',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, NULL,
   'e0000000-0000-4000-8000-000000000042',
   '7045c0d6-5260-494d-a205-4cfbaab24fea', NULL, 'Second runbook',
   'https://second.example.test/runbook', '{}',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b'),
  ('2026-01-01 00:00:16+00', '2026-01-01 00:00:16+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:16+00',
   'e0000000-0000-4000-8000-000000000043',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL, 'Retired link',
   'https://retired.example.test/', '{}',
   'd44ae570-3de4-4413-af08-f686bf5b7235');

-- 5. Home preferences for `member`: the two keys `get` never creates
--    (`plane/app/views/workspace/home.py:34-37`), so 220's PATCH reaches the
--    serializer instead of the "Preference not found" 400.
INSERT INTO workspace_home_preferences
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, user_id, key, is_enabled, config, sort_order)
VALUES
  ('2026-01-01 00:00:17+00', '2026-01-01 00:00:17+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'e0000000-0000-4000-8000-000000000050',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 'quick_tutorial', TRUE, '{}',
   65535),
  ('2026-01-01 00:00:18+00', '2026-01-01 00:00:18+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'e0000000-0000-4000-8000-000000000051',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   '78100240-9cfd-4782-8627-25ab039c6e9a', 'new_at_plane', TRUE, '{}', 65535);

-- 6. Workspace user properties in a second workspace, and a soft-deleted row in
--    `fix-workspace` (the partial unique index allows it next to admin's live
--    row, which `build.py` creates).
INSERT INTO workspace_user_properties
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, user_id, filters, display_filters, display_properties,
   rich_filters, navigation_project_limit, navigation_control_preference)
VALUES
  ('2026-01-01 00:00:19+00', '2026-01-01 00:00:19+00',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, NULL,
   'e0000000-0000-4000-8000-000000000060',
   '7045c0d6-5260-494d-a205-4cfbaab24fea',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b',
   '{}',
   '{"display_filters": {"layout": "list", "group_by": null}}',
   '{"display_properties": {"assignee": true, "state": true}}',
   '{}', 10, 'ACCORDION'),
  ('2026-01-01 00:00:20+00', '2026-01-01 00:00:20+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2026-01-01 00:00:20+00',
   'e0000000-0000-4000-8000-000000000061',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'd44ae570-3de4-4413-af08-f686bf5b7235',
   '{}',
   '{"display_filters": {"layout": "kanban", "group_by": "state"}}',
   '{"display_properties": {"assignee": false, "state": true}}',
   '{}', 10, 'TABBED');

-- 7. A `FileAsset` with **no workspace** and a non-static entity type. Route
--    186's response evaluates `Workspace.logo_url`, which returns
--    `self.logo_asset.asset_url` when the FK is set; the description branch of
--    `asset_url` reads `self.workspace.slug` (`plane/db/models/asset.py:93-104`)
--    on a NULL workspace, i.e. `None.slug` — an `AttributeError`, which DRF's
--    `Field.get_attribute` converts to `SkipField`: `logo_url` is omitted from
--    the body and the create still answers 201 (`rest_framework/fields.py:445-
--    451`).
INSERT INTO file_assets
  (created_at, updated_at, id, attributes, asset, created_by_id, updated_by_id,
   workspace_id, is_deleted, deleted_at, is_archived, comment_id, entity_type,
   external_id, external_source, is_uploaded, issue_id, page_id, project_id,
   size, storage_metadata, user_id, draft_issue_id, entity_identifier)
VALUES
  ('2026-01-01 00:00:21+00', '2026-01-01 00:00:21+00',
   'e0000000-0000-4000-8000-000000000070',
   '{"name": "orphan.txt", "type": "text/plain", "size": 16}',
   'user-00000000000000000000000000000070-orphan.txt',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   NULL, FALSE, NULL, FALSE, NULL, 'ISSUE_DESCRIPTION', NULL, NULL, TRUE,
   NULL, NULL, NULL, 16, '{}', NULL, NULL, NULL);

-- 8. `no_workspace`'s profile points at the 40-character-slug workspace, so
--    route 187's DELETE — which 500s on H05's slug rewrite — still shows the
--    `profiles.last_workspace_id` clear committed (`base.py:161-172`: the clear
--    runs before `super().destroy()`).
UPDATE profiles SET last_workspace_id = 'e0000000-0000-4000-8000-000000000001'
WHERE user_id = (SELECT id FROM users WHERE email = 'fix.lonely@example.com');
