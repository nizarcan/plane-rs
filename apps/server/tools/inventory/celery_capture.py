# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Capture the exact Celery messages Plane publishes, without a broker.

The Rust services must publish Celery-protocol messages that the Python worker
accepts byte for byte (see ``apps/server/TASKS.md``). This script records what
Celery 5.5.3 + kombu 5.5.4 actually put on the wire for representative real
tasks, so the Rust publisher can be written against evidence instead of memory.

How it works
------------
* Django is configured from ``plane.settings.production`` with throwaway
  environment values: no database, Redis, broker or bucket is contacted.
  Importing the settings and the task modules opens no sockets.
* The Plane Celery app (``plane.celery.app``) has its broker URL replaced with
  kombu's in-process ``memory://`` transport *before* any connection exists, so
  every publish ends up in a dict inside this process.
* ``.delay()`` / ``.apply_async()`` never execute a task; they only serialise
  the message and hand it to the transport. No task body runs, so no database
  or SMTP side effect happens. The task wrappers are called directly (rather
  than ``app.send_task``) because it is the wrapper's argument normalisation we
  want to observe.
* The messages are then drained from the in-memory queue and written out with
  the AMQP properties, the headers, the body as decoded JSON and the raw body
  bytes as base64.

Run it (from ``apps/api``, as ``apps/server/AGENTS.md`` says):

    DJANGO_SETTINGS_MODULE=plane.settings.production \
    SECRET_KEY=x \
    DATABASE_URL=postgresql://x:x@127.0.0.1:1/x \
    REDIS_URL=redis://127.0.0.1:1/0 \
    .venv/bin/python ../../apps/server/tools/inventory/celery_capture.py

or simply, since the script sets the same defaults itself:

    cd apps/api && .venv/bin/python ../server/tools/inventory/celery_capture.py

The output is ``apps/server/tools/inventory/celery_messages.json``.
"""

from __future__ import annotations

import base64
import json
import os
import pathlib
import sys
import uuid
from datetime import datetime, timezone
from queue import Empty
from typing import Any

# --------------------------------------------------------------------------
# 1. Locate the Django project and give it settings that cannot connect out.
# --------------------------------------------------------------------------

HERE = pathlib.Path(__file__).resolve()

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(HERE.parents[2] / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_ROOT = _oracle_api_dir()
sys.path.insert(0, str(API_ROOT))

# Throwaway values. Port 1 on 127.0.0.1 is never contacted: nothing in this
# script runs a query. They exist only so settings import without falling back
# to defaults that would point at a real service.
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "celery-capture-not-a-secret")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.setdefault("AMQP_URL", "memory://")

import django  # noqa: E402

django.setup()

import celery  # noqa: E402
import kombu  # noqa: E402
from django.core.serializers.json import DjangoJSONEncoder  # noqa: E402
from django.utils import timezone as django_timezone  # noqa: E402

from plane.celery import app  # noqa: E402

# --------------------------------------------------------------------------
# 2. Point the app at the in-memory transport.
# --------------------------------------------------------------------------

# ``memory://`` is kombu's in-process transport: publishes are appended to a
# dict and nothing leaves the process. Set the url and the read/write urls so
# no code path can fall back to the AMQP broker from Django settings.
app.conf.broker_url = "memory://"
app.conf.broker_read_url = "memory://"
app.conf.broker_write_url = "memory://"
app.conf.task_always_eager = False
app.conf.task_store_eager_result = False
app.conf.result_backend = None
app.conf.broker_connection_retry = False
app.conf.broker_connection_max_retries = 0
app.conf.task_publish_retry = False

# Force task module import (this is what the worker's loader does) so the task
# objects exist in this process.
app.loader.import_default_modules()
from plane.bgtasks.cleanup_task import delete_api_logs  # noqa: E402,F401
from plane.bgtasks.email_notification_task import send_email_notification  # noqa: E402
from plane.bgtasks.issue_activities_task import issue_activity  # noqa: E402
from plane.bgtasks.issue_version_sync import sync_issue_version  # noqa: E402
from plane.bgtasks.storage_metadata_task import get_asset_object_metadata  # noqa: E402
from plane.bgtasks.webhook_task import webhook_activity  # noqa: E402
from plane.bgtasks.workspace_invitation_task import workspace_invitation  # noqa: E402

QUEUE = app.conf.task_default_queue  # "celery"

# --------------------------------------------------------------------------
# 3. Representative payloads, copied from real call sites.
# --------------------------------------------------------------------------

WORKSPACE_ID = uuid.UUID("6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60")
USER_ID = uuid.UUID("0d1e2f30-4456-4789-9abc-def012345678")
ISSUE_ID = uuid.UUID("11111111-2222-3333-4444-555555555555")
PROJECT_ID = uuid.UUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
ASSET_ID = uuid.UUID("99999999-8888-7777-6666-555555555555")
NOW = django_timezone.now()

CASES: list[dict[str, Any]] = [
    {
        # plane/app/views/workspace/invite.py:120 -- five string-ish arguments.
        # NOTE: ``workspace.id`` is a UUID object, not a str; kombu encodes it
        # with its type envelope. The other four are plain strings.
        "case": "workspace_invitation positional arguments",
        "call_site": "plane/app/views/workspace/invite.py:120",
        "enqueue": lambda: workspace_invitation.delay(
            "invitee@example.com",
            WORKSPACE_ID,
            "eyJhbGciOiJIUzI1NiJ9.invitation-token.signature",
            "http://localhost:3000",
            "admin@example.com",
        ),
    },
    {
        # plane/app/views/asset/v2.py:219 -- kwargs only, single string uuid.
        "case": "get_asset_object_metadata kwargs",
        "call_site": "plane/app/views/asset/v2.py:219",
        "enqueue": lambda: get_asset_object_metadata.delay(asset_id=str(ASSET_ID)),
    },
    {
        # plane/app/views/issue/base.py:689 -- JSON strings produced by
        # json.dumps(..., cls=DjangoJSONEncoder) inside the kwargs dict.
        "case": "issue_activity with JSON-string payloads",
        "call_site": "plane/app/views/issue/base.py:689",
        "enqueue": lambda: issue_activity.delay(
            type="issue.activity.updated",
            requested_data=json.dumps(
                {
                    "name": "Fix the login redirect loop",
                    "priority": "high",
                    "assignees": [str(USER_ID)],
                    "target_date": "2026-11-20",
                },
                cls=DjangoJSONEncoder,
            ),
            actor_id=str(USER_ID),
            issue_id=str(ISSUE_ID),
            project_id=str(PROJECT_ID),
            current_instance=json.dumps(
                {
                    "name": "Fix login redirect",
                    "priority": "medium",
                    "assignees": [],
                    "target_date": None,
                },
                cls=DjangoJSONEncoder,
            ),
            epoch=int(NOW.timestamp()),
            notification=True,
            origin="http://localhost:3000",
        ),
    },
    {
        # plane/bgtasks/issue_version_sync.py:218 -- apply_async with kwargs and
        # a countdown, the only place Celery materialises an ``eta`` header.
        "case": "sync_issue_version apply_async with countdown",
        "call_site": "plane/bgtasks/issue_version_sync.py:218",
        "enqueue": lambda: sync_issue_version.apply_async(
            kwargs={"batch_size": 5000, "offset": 5000, "countdown": 300},
            countdown=300,
        ),
    },
    {
        # plane/bgtasks/email_notification_task.py:76 -- a nested Python dict
        # (not a JSON string) plus a list of UUIDs passed straight through.
        "case": "send_email_notification nested dict arguments",
        "call_site": "plane/bgtasks/email_notification_task.py:76",
        "enqueue": lambda: send_email_notification.delay(
            issue_id=str(ISSUE_ID),
            notification_data={
                str(USER_ID): [
                    {
                        "issue_activity": {
                            "id": str(uuid.UUID("12345678-1234-5678-1234-567812345678")),
                            "verb": "updated",
                            "field": "priority",
                            "actor": str(USER_ID),
                            "new_value": "high",
                            "old_value": "medium",
                            "old_identifier": None,
                            "new_identifier": None,
                            "activity_time": NOW.isoformat(),
                        },
                        "issue": {"id": str(ISSUE_ID), "name": "Fix the login redirect loop"},
                    }
                ]
            },
            receiver_id=str(USER_ID),
            email_notification_ids=[uuid.UUID("aaaaaaaa-0000-0000-0000-000000000001")],
        ),
    },
    {
        # plane/app/views/project/base.py:401 -- ``actor_id`` and ``event_id``
        # are UUID objects here; the API endpoints pass ``request.user.id`` (a
        # UUID) and ``project.id`` (a UUID) as well.
        "case": "webhook_activity project deleted",
        "call_site": "plane/app/views/project/base.py:401",
        "enqueue": lambda: webhook_activity.delay(
            event="project",
            verb="deleted",
            field=None,
            old_value=None,
            new_value=None,
            actor_id=USER_ID,
            slug="acme",
            current_site="http://localhost:3000",
            event_id=PROJECT_ID,
            old_identifier=None,
            new_identifier=None,
        ),
    },
    {
        # A no-argument beat task, for completeness: what a scheduled entry
        # publishes (the message carries no args at all).
        "case": "delete_api_logs no arguments",
        "call_site": "plane/celery.py:78 (beat_schedule)",
        "enqueue": lambda: delete_api_logs.delay(),
    },
]


# --------------------------------------------------------------------------
# 4. Publish, drain, record.
# --------------------------------------------------------------------------


def _drain(connection: kombu.Connection) -> list[dict[str, Any]]:
    """Pull every message out of the in-memory queue, oldest first."""
    channel = connection.default_channel
    drained = []
    while True:
        try:
            drained.append(channel._get(QUEUE))
        except Empty:
            break
    return drained


def _decode_body(raw: dict[str, Any]) -> tuple[bytes, str]:
    """Return (raw body bytes, body encoding) for a virtual-transport record."""
    body = raw["body"]
    encoding = raw["properties"].get("body_encoding", "base64")
    if isinstance(body, str):
        body = body.encode("utf-8")
    if encoding == "base64":
        return base64.b64decode(body), encoding
    return body, encoding


def main() -> int:
    connection = app.connection_for_write()
    connection.connect()
    # Sanity check: the transport really is the in-memory one.
    if connection.transport_cls != "memory":
        raise SystemExit(f"refusing to run: transport is {connection.transport_cls!r}, not memory")

    published: list[dict[str, Any]] = []
    leftover = _drain(connection)
    if leftover:
        raise SystemExit(f"queue was not empty before publishing: {len(leftover)} messages")
    for case in CASES:
        result = case["enqueue"]()
        published.append({"case": case["case"], "call_site": case["call_site"], "id": result.id})

    records = _drain(connection)
    connection.release()

    if len(records) != len(CASES):
        raise SystemExit(f"expected {len(CASES)} messages, drained {len(records)}")

    messages = []
    for meta, raw in zip(published, records):
        body_bytes, body_encoding = _decode_body(raw)
        properties = {
            k: v
            for k, v in raw["properties"].items()
            if k not in ("delivery_tag",)
        }
        message = {
            "case": meta["case"],
            "call_site": meta["call_site"],
            "task": raw["headers"]["task"],
            "task_id": meta["id"],
            "content_type": raw["content-type"],
            "content_encoding": raw["content-encoding"],
            "properties": properties,
            "headers": raw["headers"],
            "body_raw_base64": base64.b64encode(body_bytes).decode("ascii"),
            "body_bytes": len(body_bytes),
            "body_json": json.loads(body_bytes.decode(raw["content-encoding"] or "utf-8")),
        }
        messages.append(message)

    document = {
        "generated_by": "apps/server/tools/inventory/celery_capture.py",
        "generated_at": datetime.now(tz=timezone.utc).isoformat(),
        "celery_version": celery.__version__,
        "kombu_version": kombu.__version__,
        "transport": "memory (kombu in-process)",
        "app_main": app.main,
        "task_protocol": app.conf.task_protocol,
        "task_serializer": app.conf.task_serializer,
        "accept_content": list(app.conf.accept_content),
        "timezone": str(app.conf.timezone),
        "enable_utc": app.conf.enable_utc,
        "default_queue": app.conf.task_default_queue,
        "default_exchange": app.conf.task_default_exchange,
        "default_exchange_type": app.conf.task_default_exchange_type,
        "default_routing_key": app.conf.task_default_routing_key,
        "delivery_mode": 2,
        "body_encoding": messages[0]["properties"].get("body_encoding"),
        "messages": messages,
    }

    out = HERE.parent / "celery_messages.json"
    out.write_text(json.dumps(document, indent=2, sort_keys=False) + "\n", encoding="utf-8")

    print(f"celery {celery.__version__} / kombu {kombu.__version__}, protocol {app.conf.task_protocol}")
    print(f"wrote {len(messages)} messages to {out}")
    for message in messages:
        print(
            f"  {message['task']:<64} exchange={message['properties']['delivery_info']['exchange']!r} "
            f"routing_key={message['properties']['delivery_info']['routing_key']!r} "
            f"bytes={message['body_bytes']}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
