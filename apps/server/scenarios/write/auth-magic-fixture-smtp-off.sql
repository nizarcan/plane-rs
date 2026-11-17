-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `auth-magic-smtp-off.yaml`: the instance is set up with a working magic-link
-- switch, but no SMTP host. `MagicCodeProvider.__init__` checks SMTP **before**
-- the login switch and before any Redis access (`magic_code.py:67-72`), so both
-- the generate and the verify routes answer `SMTP_NOT_CONFIGURED` and neither
-- touches the code store.
--
-- The template already seeds `EMAIL_HOST = ''`, so only the instance flag and
-- the login switch need writing.

UPDATE instances
SET is_setup_done = TRUE
WHERE deleted_at IS NULL;

UPDATE instance_configurations
SET value = '1'
WHERE key = 'ENABLE_MAGIC_LINK_LOGIN';
