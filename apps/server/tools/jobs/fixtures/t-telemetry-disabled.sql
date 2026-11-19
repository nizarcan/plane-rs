-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- `push_instance_metrics` skips the push when the instance row says telemetry
-- is off (`telemetry_metrics.py:80-82`); the recorder must see zero requests.
UPDATE instances SET is_telemetry_enabled = false;
