# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Capture four real `issue_activity` publishes for the Rust golden test.

Same technique as ``tools/inventory/celery_capture.py``: Django is configured
with throwaway environment values, the Plane Celery app is pointed at kombu's
in-process transport, and every publish is a real ``task.apply_async`` through
the real wrapper. Identity (task id, origin, reply_to) is pinned so the output
is reproducible. No broker, database, Redis or bucket is contacted.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/inventory/issue_activity_capture.py

Output: ``apps/server/crates/plane-jobs/tests/fixtures/issue_activity_capture.json``.
The Rust test ``crates/plane-jobs/tests/issue_activity_capture.rs`` rebuilds
the same messages with ``plane_jobs::celery::tasks`` and compares the full
encoded message (body, ``argsrepr`` and ``kwargsrepr``) to this file.

The four shapes are TASKS.md rows 39 (issue create), 40 (issue patch),
41 (issue delete, ``subscriber=False``) and 44 (comment create). The kwargs
dicts keep the call site's keyword order because that order is what Celery's
``kwargsrepr`` renders; ``DjangoJSONEncoder`` strings are produced with the
same helper the views use.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import sys
from datetime import datetime, timezone
from queue import Empty

HERE = pathlib.Path(__file__).resolve()
API_ROOT = HERE.parents[3] / "api"
if not (API_ROOT / "manage.py").exists():  # pragma: no cover - safety net
    raise SystemExit(f"cannot find apps/api relative to {HERE}")
sys.path.insert(0, str(API_ROOT))

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "issue-activity-capture-not-a-secret")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.setdefault("AMQP_URL", "memory://")

import django  # noqa: E402

django.setup()

import celery  # noqa: E402
import kombu  # noqa: E402
import celery.app.amqp as celery_amqp  # noqa: E402
from django.core.serializers.json import DjangoJSONEncoder  # noqa: E402

from plane.celery import app  # noqa: E402

# Fixed inputs, mirrored by `crates/plane-jobs/tests/issue_activity_capture.rs`.
USER = "0d1e2f30-4456-4789-9abc-def012345678"
ISSUE = "11111111-2222-3333-4444-555555555555"
PROJECT = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
COMMENT = "55555555-6666-7777-8888-999999999999"
EPOCH = 1763217844
ORIGIN = "http://localhost:3000"
FIXED_NOW = "2026-11-15T17:24:04.144120+00:00"
FIXED_TASK_ORIGIN = "gen4242@celery-golden"
FIXED_REPLY_TO = "0f0f0f0f-0000-4000-8000-000000000000"


def jd(value: object) -> str:
    """`json.dumps(value, cls=DjangoJSONEncoder)`."""
    return json.dumps(value, cls=DjangoJSONEncoder)


def task_id(index: int) -> str:
    return f"00000000-0000-4000-8000-{index:012d}"


CASES = [
    {
        "case": "issue_activity_create",
        "call_site": "plane/app/views/issue/base.py:422",
        "kwargs": {
            "type": "issue.activity.created",
            "requested_data": jd(
                {
                    "name": "Fix the login redirect loop",
                    "priority": "high",
                    "assignee_ids": [USER],
                    "target_date": "2026-11-20",
                }
            ),
            "actor_id": USER,
            "issue_id": ISSUE,
            "project_id": PROJECT,
            "current_instance": None,
            "epoch": EPOCH,
            "notification": True,
            "origin": ORIGIN,
        },
    },
    {
        "case": "issue_activity_patch",
        "call_site": "plane/app/views/issue/base.py:689",
        "kwargs": {
            "type": "issue.activity.updated",
            "requested_data": jd({"name": "Fix login redirect", "priority": "medium"}),
            "actor_id": USER,
            "issue_id": ISSUE,
            "project_id": PROJECT,
            "current_instance": jd(
                {
                    "id": ISSUE,
                    "name": "Fix login redirect loop",
                    "priority": "high",
                    "assignee_ids": [],
                    "target_date": None,
                }
            ),
            "epoch": EPOCH,
            "notification": True,
            "origin": ORIGIN,
        },
    },
    {
        "case": "issue_activity_delete",
        "call_site": "plane/app/views/issue/base.py:730",
        "kwargs": {
            "type": "issue.activity.deleted",
            "requested_data": jd({"issue_id": ISSUE}),
            "actor_id": USER,
            "issue_id": ISSUE,
            "project_id": PROJECT,
            "current_instance": {},
            "epoch": EPOCH,
            "notification": True,
            "origin": ORIGIN,
            "subscriber": False,
        },
    },
    {
        "case": "comment_activity_create",
        "call_site": "plane/app/views/issue/comment.py:123",
        "kwargs": {
            "type": "comment.activity.created",
            "requested_data": jd(
                {
                    "id": COMMENT,
                    "comment_html": "<p>Looks good</p>",
                    "comment_json": {},
                    "access": "INTERNAL",
                    "actor": USER,
                    "issue": ISSUE,
                }
            ),
            "actor_id": USER,
            "issue_id": ISSUE,
            "project_id": PROJECT,
            "current_instance": None,
            "epoch": EPOCH,
            "notification": True,
            "origin": ORIGIN,
        },
    },
]


def configure_app() -> None:
    app.conf.broker_url = "memory://"
    app.conf.broker_read_url = "memory://"
    app.conf.broker_write_url = "memory://"
    app.conf.task_always_eager = False
    app.conf.task_store_eager_result = False
    app.conf.result_backend = None
    app.conf.broker_connection_retry = False
    app.conf.broker_connection_max_retries = 0
    app.conf.task_publish_retry = False
    app.loader.import_default_modules()


def pin_identity() -> None:
    pinned_now = datetime.fromisoformat(FIXED_NOW)
    app.now = lambda: pinned_now
    app._local.oid = FIXED_REPLY_TO
    celery_amqp.anon_nodename = lambda: FIXED_TASK_ORIGIN


def drain(connection: kombu.Connection, queue: str) -> list[dict]:
    channel = connection.default_channel
    drained = []
    while True:
        try:
            drained.append(channel._get(queue))
        except Empty:
            break
    return drained


def decode_body(raw: dict) -> bytes:
    body = raw["body"]
    encoding = raw["properties"].get("body_encoding", "base64")
    if isinstance(body, str):
        body = body.encode("utf-8")
    if encoding == "base64":
        return base64.b64decode(body)
    return body


def main() -> int:
    configure_app()
    pin_identity()
    connection = app.connection_for_write()
    connection.connect()
    if connection.transport_cls != "memory":
        raise SystemExit(f"refusing to run: transport is {connection.transport_cls!r}")
    if drain(connection, app.conf.task_default_queue):
        raise SystemExit("queue was not empty before recording")

    task = app.tasks.get("plane.bgtasks.issue_activities_task.issue_activity")
    if task is None:
        raise SystemExit("issue_activity is not registered")
    for index, case in enumerate(CASES, start=1):
        # `apply_async(kwargs=...)` is exactly what `.delay(**kwargs)` calls; the
        # dict keeps the call site's keyword order for `kwargsrepr`.
        task.apply_async(kwargs=case["kwargs"], task_id=task_id(index))

    records = drain(connection, app.conf.task_default_queue)
    connection.release()
    if len(records) != len(CASES):
        raise SystemExit(f"expected {len(CASES)} messages, drained {len(records)}")

    messages = []
    for case, raw in zip(CASES, records):
        body = decode_body(raw)
        properties = raw["properties"]
        messages.append(
            {
                "case": case["case"],
                "call_site": case["call_site"],
                "task": raw["headers"]["task"],
                "task_id": task_id(len(messages) + 1),
                "properties": {
                    "content_type": raw["content-type"],
                    "content_encoding": raw["content-encoding"],
                    "delivery_mode": properties.get("delivery_mode"),
                    "delivery_info": properties.get("delivery_info"),
                    "priority": properties.get("priority"),
                    "correlation_id": properties.get("correlation_id"),
                    "reply_to": properties.get("reply_to"),
                },
                "headers": raw["headers"],
                "body_b64": base64.b64encode(body).decode("ascii"),
                "body_bytes": len(body),
            }
        )

    out = (
        HERE.parents[2]
        / "crates"
        / "plane-jobs"
        / "tests"
        / "fixtures"
        / "issue_activity_capture.json"
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(
            {
                "generated_by": "apps/server/tools/inventory/issue_activity_capture.py",
                "celery_version": celery.__version__,
                "kombu_version": kombu.__version__,
                "transport": "memory (kombu in-process)",
                "pinned_now": FIXED_NOW,
                "pinned_origin": FIXED_TASK_ORIGIN,
                "pinned_reply_to": FIXED_REPLY_TO,
                "task_protocol": app.conf.task_protocol,
                "task_serializer": app.conf.task_serializer,
                "default_queue": app.conf.task_default_queue,
                "messages": messages,
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"wrote {len(messages)} messages to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
