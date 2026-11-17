-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- Sets `LLM_PROVIDER` to SQL NULL, which `get_llm_config` does not expect.
--
-- `get_configuration_value` returns the row's `value` verbatim when the row is
-- not encrypted (`plane/license/utils/instance_value.py:20-23`), so a NULL row
-- yields `None`, and `get_llm_config`'s next statement is
-- `SUPPORTED_PROVIDERS.get(provider_key.lower())`
-- (`plane/app/views/external/base.py:98`) — an `AttributeError` that no branch
-- catches. `plane/app/views/base.py:235-239` answers it with the family 500
-- body, not with the endpoint's own "internal error" 500.
--
-- `configure_instance` never writes a NULL (`plane/utils/instance_config_variables/core.py:216-242`
-- stores `os.environ.get(..., "openai")`), so only a hand-edited deployment can
-- reach this; the port reproduces it rather than treating NULL as "unset".
--
-- The fixture is applied before the baseline snapshot, so the row itself is not
-- part of any step's diff.
UPDATE instance_configurations
SET value = NULL, is_encrypted = FALSE, updated_at = '2026-01-01 00:00:03+00'
WHERE key = 'LLM_PROVIDER';
