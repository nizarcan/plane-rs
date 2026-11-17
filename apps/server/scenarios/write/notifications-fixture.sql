-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Notification write-scenario fixture. Applied to both differential targets
-- before the baseline snapshot (`vars: { fixture_sql: … }`), so these rows are
-- not step diffs until a request changes them.
--
-- Why it exists (`plans/NOTIFICATIONS.md` §4, "Scenario fixture"):
--
--   * `plane_fix_tpl` seeds 11 notifications (`tools/fixtures/build.py`),
--     every one with `triggered_by = member`, all live, all in `fix-workspace`,
--     and the only `snoozed_till` is three days in the future. None of the
--     manager/predicate branches below can be observed against those rows:
--   * a `triggered_by_id IS NULL` row renders `triggered_by_details: null`;
--   * a soft-deleted row must 404 on 117/118/119 and stay out of 121's scan;
--   * a row in another workspace for the same receiver proves the slug join;
--   * a **past** `snoozed_till` makes 121's `if snoozed:` branch match a row
--     that the `else` branch excludes (`base.py:251-254`);
--   * an unread **archived** row proves 121's `archived` branch;
--   * `issue_alpha` (292d4433…) is assigned to `member` but not in any of the
--     subscriber rows `plane_fix_tpl` seeds (those cover alpha:010-012), so
--     `type=watching` matches nothing until the `issue_subscribers` row below;
--   * two extra issue ids split `type=watching` from `type=assigned`:
--     839b5cb8… (ALPHA-030) is assigned to `member`, not subscribed,
--     d432015f… (ALPHA-003) is subscribed, not assigned;
--   * `member2`'s workspace membership is made inactive so the permission
--     decorator's `is_active` test is observable with an authenticatable user
--     (`plane/app/permissions/base.py:59-66`).
--
-- Ids are fixed and documented here; the scenarios' `vars` repeat them.
--
--   workspace       a91c3d80-5ed0-40f3-92a8-a28c015f4388  (fix-workspace)
--   workspace ws2   7045c0d6-5260-494d-a205-4cfbaab24fea  (fix-second)
--   member          78100240-9cfd-4782-8627-25ab039c6e9a
--   member2         bc437914-c250-4c9f-ac69-85003fdf6205
--   project alpha   b0063033-54f9-49d8-b5f3-6b7a1551c6a6
--   issue_alpha     292d4433-edc1-4f05-9cca-c6954821c078

-- ---------------------------------------------------------------------------
-- Notifications
-- ---------------------------------------------------------------------------
INSERT INTO notifications
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, project_id, data, entity_identifier, entity_name, title,
   message, message_html, message_stripped, sender, triggered_by_id,
   receiver_id, read_at, snoozed_till, archived_at)
VALUES
  -- Null `triggered_by`: the nested serializer renders `null`.
  ('2026-01-02 00:00:01+00', '2026-01-02 00:00:01+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000001',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture null trigger', NULL, '<p>Fixture null trigger</p>',
   'Fixture null trigger', 'in_app:issue_activities:created', NULL,
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL),
  -- Soft-deleted: every action is the manager's 404; 121's scan skips it.
  ('2026-01-02 00:00:02+00', '2026-01-02 00:00:02+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, '2026-01-03 00:00:00+00',
   'b8000000-0000-4000-8000-000000000002',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture soft deleted', NULL, '<p>Fixture soft deleted</p>',
   'Fixture soft deleted', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL),
  -- Another workspace for the same receiver: the URL slug scopes it away.
  ('2026-01-02 00:00:03+00', '2026-01-02 00:00:03+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000003',
   '7045c0d6-5260-494d-a205-4cfbaab24fea',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture ws2 row', NULL, '<p>Fixture ws2 row</p>',
   'Fixture ws2 row', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL),
  -- A past snooze: 121's `snoozed=true` includes it, the default excludes it.
  ('2026-01-02 00:00:04+00', '2026-01-02 00:00:04+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000004',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture past snooze', NULL, '<p>Fixture past snooze</p>',
   'Fixture past snooze', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   '2026-01-01 00:00:00+00', NULL),
  -- Unread but archived: 121's `archived=true` branch marks it.
  ('2026-01-02 00:00:05+00', '2026-01-02 00:00:05+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000005',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture archived unread', NULL, '<p>Fixture archived unread</p>',
   'Fixture archived unread', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   '2026-01-04 00:00:00+00'),
  -- ALPHA-030: assigned to `member`, not subscribed.
  ('2026-01-02 00:00:06+00', '2026-01-02 00:00:06+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000006',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '839b5cb8-879f-450f-abaf-4ea09c9e09e4', 'issue',
   'Fixture assigned only', NULL, '<p>Fixture assigned only</p>',
   'Fixture assigned only', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL),
  -- ALPHA-003: subscribed, not assigned.
  ('2026-01-02 00:00:07+00', '2026-01-02 00:00:07+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000007',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   'd432015f-5a0f-4904-a2eb-d8bfea948a29', 'issue',
   'Fixture subscribed only', NULL, '<p>Fixture subscribed only</p>',
   'Fixture subscribed only', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL, NULL),
  -- Two future snoozes: the form body `snoozed=false` (a truthy string) marks
  -- 0009, a JSON `snoozed: false` takes the else branch and leaves 0010 unread
  -- (`base.py:251-254`).
  ('2026-01-02 00:00:08+00', '2026-01-02 00:00:08+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000009',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture form truthy snooze', NULL, '<p>Fixture form truthy snooze</p>',
   'Fixture form truthy snooze', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   '2027-01-01 00:00:00+00', NULL),
  ('2026-01-02 00:00:09+00', '2026-01-02 00:00:09+00',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL, NULL,
   'b8000000-0000-4000-8000-000000000010',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6', '{}',
   '292d4433-edc1-4f05-9cca-c6954821c078', 'issue',
   'Fixture json false snooze', NULL, '<p>Fixture json false snooze</p>',
   'Fixture json false snooze', 'in_app:issue_activities:created',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   '2027-02-01 00:00:00+00', NULL);

-- ---------------------------------------------------------------------------
-- `member` subscribes to issue_alpha, which it is also assigned to, so
-- `type=watching` has a row to mark.
-- ---------------------------------------------------------------------------
INSERT INTO issue_subscribers
  (created_at, updated_at, id, created_by_id, issue_id, project_id,
   subscriber_id, updated_by_id, workspace_id, deleted_at)
VALUES
  ('2026-01-01 00:00:00+00', '2026-01-01 00:00:00+00',
   'b9000000-0000-4000-8000-000000000001',
   '78100240-9cfd-4782-8627-25ab039c6e9a',
   '292d4433-edc1-4f05-9cca-c6954821c078',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   '78100240-9cfd-4782-8627-25ab039c6e9a', NULL,
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', NULL);

-- ---------------------------------------------------------------------------
-- `member2`'s workspace membership becomes inactive: the user still
-- authenticates, so `allow_permission(WORKSPACE)`'s `is_active` predicate is
-- the 403 (`plane/app/permissions/base.py:59-66`).
-- ---------------------------------------------------------------------------
UPDATE workspace_members
   SET is_active = FALSE
 WHERE workspace_id = 'a91c3d80-5ed0-40f3-92a8-a28c015f4388'
   AND member_id = 'bc437914-c250-4c9f-ac69-85003fdf6205';
