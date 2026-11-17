-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `scenarios/auth-session.yaml`'s instance setup.
--
-- The template database has one `instances` row with `is_setup_done = false`
-- (`dev/bootstrap.sh`). The row is normally completed by
-- `POST /api/instances/admins/sign-up/`, which is a Phase 7 route; the session
-- scenario needs the gate open and nothing else, so it flips the flag here.
--
-- No users are inserted: the scenario creates its own through
-- `POST /auth/sign-up/`.

UPDATE instances SET is_setup_done = true;
