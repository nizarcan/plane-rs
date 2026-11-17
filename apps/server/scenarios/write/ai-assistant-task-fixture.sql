-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Gives the AI assistant a (fake) API key so the handler gets past
-- `get_llm_config()` and reaches `Task is required`.
--
-- `configure_instance` seeds every instance-configuration row from the
-- environment (`plane/license/management/commands/configure_instance.py:28-38`);
-- the AI block is `plane/utils/instance_config_variables/core.py:216-242`, so
-- the dev template already carries the three rows:
--
--   LLM_API_KEY  ''             is_encrypted = TRUE   (encrypt_data("") is "")
--   LLM_PROVIDER 'openai'       is_encrypted = FALSE
--   LLM_MODEL    'gpt-4o-mini'  is_encrypted = FALSE
--
-- An UPDATE therefore suffices and keeps the row's id and category. The value
-- is deliberately not a real credential and never leaves the machine: every
-- step in the scenario stops before the HTTP call.
--
-- With `SKIP_ENV_VAR` at its default (`1`, `plane/settings/common.py:401`)
-- `get_configuration_value` reads this row instead of the environment
-- (`plane/license/utils/instance_value.py:17-39`), so a shell `LLM_API_KEY`
-- cannot change the answer.
--
-- The fixture is applied before the baseline snapshot, so the row itself is not
-- part of any step's diff.
UPDATE instance_configurations
SET value = 'sk-not-a-real-key', is_encrypted = FALSE, updated_at = '2026-01-01 00:00:02+00'
WHERE key = 'LLM_API_KEY';
