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


def main() -> int:
    task_name, kwargs_json = sys.argv[1], sys.argv[2]
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

    import django

    django.setup()

    from django.utils.module_loading import import_string

    task = import_string(task_name)
    kwargs = json.loads(kwargs_json)
    result = task.apply(kwargs=kwargs, throw=False)
    print(json.dumps({"status": result.status, "result": str(result.result)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
