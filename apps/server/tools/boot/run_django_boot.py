#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Run one Django boot step in-process, with the network patched out.

`tools/boot/compare_boot.py` compares the boot steps' database, Redis and S3
effects with `plane-admin`'s. The Django side must not reach GitHub (the
machine has no outbound internet), so this wrapper patches
`requests.get` in `register_instance` to a local fake and records the Celery
`push_instance_metrics.delay()` call instead of publishing it (the Rust side
deliberately does not publish it, owner decision 5).

    PLANE_DB=t_p8boot_boot_a DATABASE_URL=... python run_django_boot.py \\
        register_instance p8boot-signature

The wrapper prints the management command's output on stdout, exactly as
`manage.py` would, and the metrics call count on stderr.
"""

import os
import sys
from pathlib import Path

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
os.chdir(API_DIR)
sys.path.insert(0, str(API_DIR))

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

import django  # noqa: E402

django.setup()

from django.core.management import call_command  # noqa: E402
from unittest import mock  # noqa: E402


class FakeResponse:
    def __init__(self, payload):
        self._payload = payload
        self.status_code = 200

    def raise_for_status(self):
        return None

    def json(self):
        return self._payload


def main() -> int:
    step = sys.argv[1]
    if step == "register_instance":
        import plane.license.management.commands.register_instance as command

        payload = {"tag_name": "v9.9.9"}
        metrics = mock.Mock()
        with (
            mock.patch.object(command.requests, "get", return_value=FakeResponse(payload)),
            mock.patch.object(command, "push_instance_metrics", metrics),
        ):
            call_command("register_instance", sys.argv[2])
        print(f"PUSH_INSTANCE_METRICS_CALLS={metrics.delay.call_count}", file=sys.stderr)
        return 0
    if step == "seed_user":
        from django.contrib.auth.hashers import make_password

        from plane.db.models import User

        identifier, email, password = sys.argv[2], sys.argv[3], sys.argv[4]
        User.objects.filter(email=email).delete()
        User.objects.create(
            id=identifier,
            username="p8boot-user",
            email=email,
            display_name="P8 Boot",
            password=make_password(password),
            is_active=False,
        )
        return 0
    if step == "configure_instance":
        call_command("configure_instance")
        return 0
    if step == "create_bucket":
        call_command("create_bucket")
        return 0
    if step == "clear_cache":
        call_command("clear_cache")
        return 0
    raise SystemExit(f"unknown step {step!r}")


if __name__ == "__main__":
    sys.exit(main())
