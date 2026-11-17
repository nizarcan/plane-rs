-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `auth-magic-disabled.yaml`: SMTP is configured, but `ENABLE_MAGIC_LINK_LOGIN`
-- is the seeded `'0'` (`plane/utils/instance_config_variables/core.py:22-23`;
-- `ENV.md` §5). The provider's second gate (`magic_code.py:74-79`) then answers
-- `MAGIC_LINK_LOGIN_DISABLED`, which is why the template default is used
-- unchanged for that key.

UPDATE instances
SET is_setup_done = TRUE
WHERE deleted_at IS NULL;

UPDATE instance_configurations
SET value = '127.0.0.1'
WHERE key = 'EMAIL_HOST';

UPDATE instance_configurations
SET value = '0'
WHERE key = 'ENABLE_MAGIC_LINK_LOGIN';
