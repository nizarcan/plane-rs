-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Flips `DISABLE_WORKSPACE_CREATION` to `"1"` for the two scenarios that need
-- the gate (`plane/app/views/workspace/base.py:84-97`).
--
-- The dev template database already carries the row: `dev/bootstrap.sh` runs
-- `configure_instance`, which loads every instance configuration from the
-- environment, so the key exists with the environment's value (the dev stack
-- leaves it unset, i.e. `"0"`). An UPDATE therefore suffices, and it keeps the
-- row's id and category.
--
-- With `SKIP_ENV_VAR` at its default (`1`, `plane/settings/common.py:401`)
-- `get_configuration_value` reads this row instead of the environment
-- (`plane/license/utils/instance_value.py:17-39`); `workspaces-create-env.yaml`
-- sets `SKIP_ENV_VAR=0` to prove the opposite branch ignores the row.
--
-- The fixture is applied before the baseline snapshot, so the row itself is not
-- part of any step's diff.
UPDATE instance_configurations
SET value = '1', is_encrypted = FALSE, updated_at = '2026-01-01 00:00:01+00'
WHERE key = 'DISABLE_WORKSPACE_CREATION';
