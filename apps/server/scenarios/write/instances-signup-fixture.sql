-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Fixture for `instances-signup.yaml`: the first-boot state sign-up requires.
--
-- The template's instance is already configured and has two `instance_admins`
-- rows, and `InstanceAdminEndpoint`'s guard is `instance.is_setup_done or
-- InstanceAdmin.objects.exists()` (`plane/license/api/views/admin.py:219-249`),
-- so the rows have to go for the success path to be reachable. Both statements
-- are part of the baseline, never a step diff.

DELETE FROM instance_admins;

UPDATE instances
SET is_setup_done = false,
    instance_name = '',
    is_telemetry_enabled = true,
    updated_at = '2026-01-01T00:00:00+00:00';
