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
      '{"slug":"acme", ...}' '[]' [retries]

The third argument is the positional `args` array (default `[]`); a task whose
call sites are positional passes them there instead of in `kwargs`.

The optional fourth argument is the task's `request.retries` (a `difftest task`
case's `attempts.number - 1`). With it the harness runs **one** attempt, the
way a worker would run the Nth delivery, instead of `Task.apply()`'s eager
loop: `apply()` re-executes a `Retry` signature with `retries + 1` until the
task succeeds or exhausts its budget, which hides which attempt produced which
effect. The single attempt still uses Celery's own tracer, so the task body,
the autoretry wrapper, `self.request.retries` and the resulting `RETRY` state
are Celery's.

Prints one JSON line: the task status, the string form of its result, and (for
a retry) the countdown Celery asked for.
"""

import contextlib
import io
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


def freeze_metrics_clock() -> None:
    """Pin the OTel SDK's metric collection timestamp (test only).

    `push_instance_metrics` exports the protobuf body `difftest task` compares
    byte for byte, and every `NumberDataPoint.time_unix_nano` is the SDK's
    `time_ns()` at collection. The Rust handler reads the same value from
    `PLANE_RS_DIFFTEST_METRICS_NS`; the harness sets both variables and never a
    deployment.
    """
    raw = os.environ.get("PLANE_DIFFTEST_METRICS_NS")
    if not raw:
        return

    import opentelemetry.sdk.metrics._internal.metric_reader_storage as storage

    value = int(raw)
    storage.time_ns = lambda: value


def apply_single_attempt(task, args, kwargs, retries):
    """Run one Celery attempt with `request.retries = retries`, no eager loop.

    This mirrors `celery.app.task.Task.apply()` (`is_eager=True`, the tracer,
    the `ExceptionInfo` unwrapping and the state derivation) except that a
    `Retry` carrying a signature is returned as the `RETRY` state instead of
    being applied again with `retries + 1`. `apply()`'s recursion is what a
    real worker's broker round trip does; a differential case needs the single
    attempt whose `retry_count`/`run_at` it is asserting.
    """
    import socket
    import uuid

    from billiard.einfo import ExceptionInfo, ExceptionWithTraceback
    from celery import states
    from celery.app.trace import build_tracer
    from celery.exceptions import Retry

    app = task._get_app()
    task_id = str(uuid.uuid4())
    request = {
        "id": task_id,
        "task": task.name,
        "retries": retries,
        "is_eager": True,
        "logfile": None,
        "loglevel": 0,
        "hostname": socket.gethostname(),
        "callbacks": None,
        "errbacks": None,
        "headers": None,
        "ignore_result": False,
        "delivery_info": {
            "is_eager": True,
            "exchange": None,
            "routing_key": None,
            "priority": None,
        },
    }
    tracer = build_tracer(task.name, task, eager=True, propagate=False, app=app)
    traced = tracer(task_id, args, kwargs, request)
    retval = traced.retval
    if isinstance(retval, ExceptionInfo):
        retval = retval.exception
        if isinstance(retval, ExceptionWithTraceback):
            retval = retval.exc
    state = states.SUCCESS if traced.info is None else traced.info.state
    countdown = None
    if isinstance(retval, Retry) and isinstance(retval.when, (int, float)):
        # `retry_backoff=600, retry_jitter=True` makes `when` the uniform
        # countdown `get_exponential_backoff_interval` picked, in seconds.
        countdown = float(retval.when)
    return state, retval, countdown


def main() -> int:
    task_name, kwargs_json = sys.argv[1], sys.argv[2]
    args_json = sys.argv[3] if len(sys.argv) > 3 else "[]"
    retries_arg = sys.argv[4] if len(sys.argv) > 4 else ""
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

    import django

    django.setup()

    allow_loopback_for_difftest()
    freeze_metrics_clock()

    from django.utils.module_loading import import_string

    task = import_string(task_name)
    kwargs = json.loads(kwargs_json)
    args = json.loads(args_json)
    # `notification_task.py` prints its swallowed exception to stdout (`:673`),
    # as do a few other tasks; keep the harness's one-line JSON protocol clean.
    swallowed = io.StringIO()
    result = None
    outcome = None
    with contextlib.redirect_stdout(swallowed):
        if retries_arg != "":
            outcome = apply_single_attempt(task, args, kwargs, int(retries_arg))
        else:
            result = task.apply(args=args, kwargs=kwargs, throw=False)
    if text := swallowed.getvalue():
        sys.stderr.write(text)
    if outcome is not None:
        status, retval, countdown = outcome
        print(
            json.dumps(
                {
                    "status": status,
                    "exc_type": type(retval).__name__ if status != "SUCCESS" else "",
                    "result": str(retval),
                    "countdown": countdown,
                }
            )
        )
        return 0
    print(
        json.dumps(
            {
                "status": result.status,
                "exc_type": type(result.result).__name__ if result.failed() else "",
                "result": str(result.result),
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
