-- Copyright (c) 2023-present Plane Software, Inc. and contributors
-- SPDX-License-Identifier: AGPL-3.0-only
-- See the LICENSE file for details.
--
-- 300 extra workspaces (on top of plane_fix_tpl's fixture) make the OTLP
-- protobuf exceed the HTTP/2 peer's initial 65 KiB stream window, so the gRPC
-- transport has to split the message into flow-control-sized DATA frames.
INSERT INTO workspaces
    (id, created_at, updated_at, name, slug, owner_id, timezone, background_color)
SELECT
    ('00000000-0000-4000-8000-' || lpad(i::text, 12, '0'))::uuid,
    now() + (i * interval '1 second'),
    now() + (i * interval '1 second'),
    'Large ' || i,
    't-large-' || i,
    (SELECT id FROM users WHERE is_bot = false ORDER BY created_at LIMIT 1),
    'UTC',
    '#000000'
FROM generate_series(1, 300) AS i;
