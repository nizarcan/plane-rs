#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Run Django's version-backfill Celery chain in-process.

`manage.py sync_issue_version` only enqueues `schedule_issue_version`, which
enqueues the first `sync_issue_version` batch, which re-enqueues the next batch
with `countdown`. This script is the chain with `countdown=0`: it calls
`task.apply()` for offset 0, batch_size, 2*batch_size, ... until the task's own
`end_offset < total_issues_count` condition is false.

Usage (with the dev environment sourced and `PLANE_DB` pointing at the clone):

    PLANE_DB=t_p8a3_bf_a tools/jobs/run_backfill_chain.py \
        plane.bgtasks.issue_version_sync.sync_issue_version 2
"""

import os
import sys

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

import django  # noqa: E402

django.setup()

from django.utils.module_loading import import_string  # noqa: E402

from plane.db.models import Issue  # noqa: E402


def main() -> int:
    task_name, batch_size = sys.argv[1], int(sys.argv[2])
    task = import_string(task_name)
    offset = 0
    while True:
        total = Issue.objects.count()
        if total == 0 or offset >= total:
            break
        result = task.apply(
            kwargs={"batch_size": batch_size, "offset": offset, "countdown": 0},
            throw=False,
        )
        print(f"django {task_name} offset={offset} status={result.status}")
        if result.status != "SUCCESS":
            print(f"django failure: {result.result!r}")
            return 1
        offset = min(offset + batch_size, total)
    return 0


if __name__ == "__main__":
    sys.exit(main())
