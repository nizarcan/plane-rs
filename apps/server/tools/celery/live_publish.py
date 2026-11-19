# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Publish a representative case to a real broker, with pinned identity.

Used by ``live_check.sh`` to produce the Python side of the comparison. The
task id, ``origin``, ``reply_to`` and publish instant are the same values
``record.py`` and ``plane-jobs``'s ``publish_case`` example use, so the Python
and Rust messages for one case must be byte-identical.

    cd apps/api
    .venv/bin/python ../../apps/server/tools/celery/live_publish.py \
        --url amqp://plane:plane@127.0.0.1:55672/t_celery_live --case process_logs
"""

from __future__ import annotations

import argparse
import os
import pathlib
import sys
from datetime import datetime

HERE = pathlib.Path(__file__).resolve()

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(HERE.parents[2] / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_ROOT = _oracle_api_dir()
sys.path.insert(0, str(API_ROOT))
sys.path.insert(0, str(HERE.parent))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True, help="amqp:// URL of a disposable vhost")
    parser.add_argument("--case", help="case id from cases.py")
    parser.add_argument("--all", action="store_true", help="publish every case")
    args = parser.parse_args()

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "celery-live-not-a-secret")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ["AMQP_URL"] = args.url

    import django

    django.setup()

    import celery.app.amqp as celery_amqp

    from plane.celery import app

    import cases

    app.conf.broker_url = args.url
    app.conf.broker_read_url = args.url
    app.conf.broker_write_url = args.url
    app.conf.task_always_eager = False
    app.conf.result_backend = None
    app.loader.import_default_modules()

    pinned_now = datetime.fromisoformat(cases.FIXED_NOW)
    app.now = lambda: pinned_now
    app._local.oid = cases.FIXED_REPLY_TO
    celery_amqp.anon_nodename = lambda: cases.FIXED_ORIGIN

    numbered = cases.numbered()
    if args.all:
        selected = numbered
    else:
        if not args.case:
            raise SystemExit("--case or --all is required")
        selected = [case for case in numbered if case["case"] == args.case]
        if not selected:
            raise SystemExit(f"unknown case {args.case!r}")

    for case in selected:
        task = app.tasks.get(case["task"])
        if task is None:
            raise SystemExit(f"task {case['task']!r} is not registered")
        task.apply_async(
            args=tuple(case.get("args", ())),
            kwargs=case.get("kwargs", {}),
            countdown=case.get("countdown"),
            task_id=case["id"],
        )
        print(f"published {case['case']} task_id={case['id']}")

    # `apply_async` is fire-and-forget (no publisher confirms, like Celery);
    # close the pooled connections so every frame is flushed before the drain
    # runs.
    app.pool.force_close_all()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
