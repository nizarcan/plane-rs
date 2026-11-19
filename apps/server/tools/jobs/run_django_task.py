#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run one Celery task in-process, the reference side of `difftest task`.

`difftest task` (A = Celery) executes the real Django task with
`task.apply(kwargs=...)`: the task body, the ORM and every side effect run
exactly as they do in a Celery worker, without a broker or a worker process.
That is `plans/CUTOVER.md` §2's "A = Django `task.apply()` in-process".

Usage (the harness passes the environment from dev/django-env.sh):

  .venv/bin/python ../server/tools/jobs/run_django_task.py \
      plane.bgtasks.recent_visited_task.recent_visited_task \
      '{"slug":"acme", ...}'

Prints one JSON line: the task status and the string form of its result.
"""

import json
import os
import sys


def allow_loopback_for_difftest() -> None:
    """Let the harness's local fake HTTP server be reachable (test only).

    `difftest task` runs a recorder on 127.0.0.1 so the crawl/unfurl path can
    be compared without any traffic leaving the machine. Django's SSRF guards
    block loopback by design and the crawl task passes no allow-list, so the
    harness sets `PLANE_DIFFTEST_ALLOW_LOOPBACK=1` and only then rewrites the
    two block checks for loopback addresses. Nothing in `apps/api` changes, and
    no deployment sets the variable.
    """
    if os.environ.get("PLANE_DIFFTEST_ALLOW_LOOPBACK") != "1":
        return

    import ipaddress

    from plane.bgtasks import work_item_link_task
    from plane.utils import ip_address as ip_module

    real_is_blocked = ip_module.is_blocked_ip

    def is_blocked_ip(ip):
        if ipaddress.ip_address(ip).is_loopback:
            return False
        return real_is_blocked(ip)

    # `resolve_and_validate` reads the module global; the task imported the
    # name directly, so patch both bindings.
    ip_module.is_blocked_ip = is_blocked_ip
    work_item_link_task.is_blocked_ip = is_blocked_ip


def main() -> int:
    task_name, kwargs_json = sys.argv[1], sys.argv[2]
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

    import django

    django.setup()

    allow_loopback_for_difftest()

    from django.utils.module_loading import import_string

    task = import_string(task_name)
    kwargs = json.loads(kwargs_json)
    result = task.apply(kwargs=kwargs, throw=False)
    print(json.dumps({"status": result.status, "result": str(result.result)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
