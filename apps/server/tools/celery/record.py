# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the exact Celery messages for the representative cases.

No broker and no database are contacted: the Plane Celery app is pointed at
kombu's in-process ``memory://`` transport, exactly like
``tools/inventory/celery_capture.py``. Every publish is a real
``task.apply_async(...)`` through the real task wrapper, so the argument
normalisation, ``saferepr`` and serialisation are Celery's own.

Identity is pinned so the output is reproducible:

* each case's task id comes from ``cases.numbered()`` (``task_id=`` option);
* ``origin`` is patched through ``celery.app.amqp.anon_nodename``;
* ``reply_to`` is pinned by setting ``app._local.oid`` (``app.thread_oid``);
* ``app.now`` returns ``cases.FIXED_NOW`` so a ``countdown=`` publish
  produces a fixed ``eta``.

Run from ``apps/api`` (or anywhere; the script adds the venv's Django project
to ``sys.path``):

    cd apps/api
    .venv/bin/python ../../apps/server/tools/celery/record.py

The output is ``apps/server/tools/celery/golden/messages.json``.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import sys
from datetime import datetime
from typing import Any

HERE = pathlib.Path(__file__).resolve()
API_ROOT = HERE.parents[3] / "api"  # apps/server/tools/celery -> apps/api
if not (API_ROOT / "manage.py").exists():  # pragma: no cover - safety net
    raise SystemExit(f"cannot find apps/api relative to {HERE}")
sys.path.insert(0, str(API_ROOT))
sys.path.insert(0, str(HERE.parent))

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "celery-record-not-a-secret")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.setdefault("AMQP_URL", "memory://")

import django  # noqa: E402

django.setup()

import celery  # noqa: E402
import kombu  # noqa: E402
import celery.app.amqp as celery_amqp  # noqa: E402

from plane.celery import app  # noqa: E402

import cases  # noqa: E402


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


def pin_identity() -> datetime:
    pinned_now = datetime.fromisoformat(cases.FIXED_NOW)
    app.now = lambda: pinned_now
    app._local.oid = cases.FIXED_REPLY_TO
    celery_amqp.anon_nodename = lambda: cases.FIXED_ORIGIN
    return pinned_now


def drain(connection: kombu.Connection, queue: str) -> list[dict[str, Any]]:
    channel = connection.default_channel
    drained = []
    from queue import Empty

    while True:
        try:
            drained.append(channel._get(queue))
        except Empty:
            break
    return drained


def decode_body(raw: dict[str, Any]) -> bytes:
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
    numbered = cases.numbered()

    connection = app.connection_for_write()
    connection.connect()
    if connection.transport_cls != "memory":
        raise SystemExit(f"refusing to run: transport is {connection.transport_cls!r}")
    if drain(connection, app.conf.task_default_queue):
        raise SystemExit("queue was not empty before recording")

    for case in numbered:
        task = app.tasks.get(case["task"])
        if task is None:
            raise SystemExit(f"task {case['task']!r} is not registered")
        task.apply_async(
            # `.delay(*args)` builds a tuple; `apply_async(args=[...])` would
            # keep a list and change `argsrepr` (`['a']` vs `('a',)`).
            args=tuple(case.get("args", ())),
            kwargs=case.get("kwargs", {}),
            countdown=case.get("countdown"),
            task_id=case["id"],
        )

    records = drain(connection, app.conf.task_default_queue)
    connection.release()
    if len(records) != len(numbered):
        raise SystemExit(f"expected {len(numbered)} messages, drained {len(records)}")

    messages = []
    for case, raw in zip(numbered, records):
        body = decode_body(raw)
        properties = raw["properties"]
        messages.append(
            {
                "case": case["case"],
                "call_site": case["call_site"],
                "task": raw["headers"]["task"],
                "task_id": case["id"],
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

    document = {
        "generated_by": "apps/server/tools/celery/record.py",
        "celery_version": celery.__version__,
        "kombu_version": kombu.__version__,
        "transport": "memory (kombu in-process)",
        "pinned_now": cases.FIXED_NOW,
        "pinned_origin": cases.FIXED_ORIGIN,
        "pinned_reply_to": cases.FIXED_REPLY_TO,
        "task_protocol": app.conf.task_protocol,
        "task_serializer": app.conf.task_serializer,
        "default_queue": app.conf.task_default_queue,
        "messages": messages,
    }

    out = HERE.parent / "golden" / "messages.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        json.dumps(document, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"celery {celery.__version__} / kombu {kombu.__version__}")
    print(f"wrote {len(messages)} messages to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
