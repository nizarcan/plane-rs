-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Deactivation fixture (`plans/USERS_ME.md` §3, set B).
--
-- `nonmember` is the account the destructive step deactivates:
--
--   * the admin of `fix-second` and of project `theta`, and the only *admin*
--     there. `UserEndpoint.deactivate`'s annotation-based guard would refuse
--     that shape if it worked; both annotations are dead constants
--     (`plans/USERS_ME.md` §4), so the request must proceed. A live project
--     membership in `alpha` is added so the project scan flips more than one
--     row;
--   * one live and one soft-deleted workspace invitation address the H04
--     sweep (`WorkspaceMemberInvite.objects.filter(email=…).delete()`); a
--     second live invitation for another address proves the filter;
--   * a second session row, copied from the fixture's own nonmember session
--     (whose signed payload is not reproducible here), proves
--     `Session.objects.filter(user_id=…).delete()` removes **every** row and
--     not just the request's.
--
-- Ids are `e1000000-…`; timestamps are fixed so the rows are identical on both
-- sides and outside the harness's "now" window.

-- 1. A live project membership in `alpha` (nonmember is only in `theta` today).
INSERT INTO project_members
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   project_id, workspace_id, member_id, comment, role, view_props,
   default_props, preferences, sort_order, is_active)
VALUES
  ('2026-01-01 00:00:21+00', '2026-01-01 00:00:21+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e1000000-0000-4000-8000-000000000001',
   'b0063033-54f9-49d8-b5f3-6b7a1551c6a6',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388',
   'e15c61cc-f495-4e69-a88f-9c2cf5cdca7b', NULL, 5, '{}', '{}', '{}', 65535,
   TRUE);

-- 2. Invitations: the sweep touches live rows for exactly `user.email`.
INSERT INTO workspace_member_invites
  (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id,
   workspace_id, email, accepted, token, message, responded_at, role)
VALUES
  ('2026-01-01 00:00:22+00', '2026-01-01 00:00:22+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e1000000-0000-4000-8000-000000000011',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'fix.nonmember@example.com', FALSE,
   'e100000000000000000000000000000000000000000000000000000000000011',
   NULL, NULL, 5),
  -- Control: a different address must survive the sweep.
  ('2026-01-01 00:00:23+00', '2026-01-01 00:00:23+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL, NULL,
   'e1000000-0000-4000-8000-000000000012',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'fix.guest@example.com', FALSE,
   'e100000000000000000000000000000000000000000000000000000000000012',
   NULL, NULL, 5),
  -- Already deleted: the sweep's `deleted_at IS NULL` predicate keeps its
  -- original timestamp.
  ('2026-01-01 00:00:24+00', '2026-01-01 00:00:24+00',
   'd44ae570-3de4-4413-af08-f686bf5b7235', NULL,
   '2025-12-01 00:00:00+00',
   'e1000000-0000-4000-8000-000000000013',
   'a91c3d80-5ed0-40f3-92a8-a28c015f4388', 'fix.nonmember@example.com', FALSE,
   'e100000000000000000000000000000000000000000000000000000000000013',
   NULL, NULL, 5);

-- 3. A second session for `nonmember`, copied from the deterministic fixture
--    row so the signed payload stays valid on both targets. Only the key and
--    the expiry differ; the handler must delete this row too.
INSERT INTO sessions (session_key, session_data, expire_date, user_id, device_info)
SELECT
  'e1000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff',
  session_data, expire_date + interval '1 day', user_id, device_info
FROM sessions
WHERE session_key = '1f63a433ad114a38858866feb53deea5000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000';
