# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Drain a Celery queue with kombu and print two views of every delivery.

* ``raw`` keeps the AMQP properties, the application headers and the body
  bytes (base64), exactly what a byte comparison needs;
* ``decoded`` is the harness-compatible view: task name, headers,
  properties that are meaningful, and the JSON body split into
  ``args``/``kwargs``/``body_extra`` (kombu's UUID envelopes stay as JSON
  objects, matching ``plane-testkit``'s decoder).

The channel is switched to ``auto_decode = False`` so the body is the raw
bytes rather than py-amqp's UTF-8 decoded string.

    .venv/bin/python ../../apps/server/tools/celery/drain.py --url amqp://... > drained.json
"""

from __future__ import annotations

import argparse
import base64
import json

import kombu


def drain(url: str, queue: str) -> list[dict]:
    connection = kombu.Connection(url)
    channel = connection.default_channel
    channel.auto_decode = False
    deliveries = []
    while True:
        message = channel.basic_get(queue)
        if message is None:
            break
        body = message.body
        if isinstance(body, str):
            body = body.encode("utf-8")
        properties = message.properties or {}
        headers = message.headers or {}
        try:
            parsed = json.loads(body.decode("utf-8"))
            args, kwargs, extra = parsed[0], parsed[1], parsed[2]
        except (UnicodeDecodeError, json.JSONDecodeError):
            args, kwargs, extra = json.loads(json.dumps(body.decode("utf-8", "replace"))), None, None
        deliveries.append(
            {
                "raw": {
                    "task": headers.get("task"),
                    "task_id": headers.get("id"),
                    "headers": headers,
                    "properties": {
                        "content_type": properties.get("content_type"),
                        "content_encoding": properties.get("content_encoding"),
                        "delivery_mode": properties.get("delivery_mode"),
                        "priority": properties.get("priority"),
                        "correlation_id": properties.get("correlation_id"),
                        "reply_to": properties.get("reply_to"),
                        "delivery_info": {
                            "exchange": (message.delivery_info or {}).get("exchange"),
                            "routing_key": (message.delivery_info or {}).get("routing_key"),
                        },
                    },
                    "body_b64": base64.b64encode(body).decode("ascii"),
                },
                "decoded": {
                    "task": headers.get("task"),
                    "headers": headers,
                    "properties": {
                        "content_type": properties.get("content_type"),
                        "content_encoding": properties.get("content_encoding"),
                        "delivery_mode": properties.get("delivery_mode"),
                        "priority": properties.get("priority"),
                    },
                    "args": args,
                    "kwargs": kwargs,
                    "body_extra": extra,
                },
            }
        )
        channel.basic_ack(message.delivery_info["delivery_tag"])
    connection.release()
    return deliveries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", required=True)
    parser.add_argument("--queue", default="celery")
    args = parser.parse_args()
    deliveries = drain(args.url, args.queue)
    print(json.dumps(deliveries, indent=2, sort_keys=True, default=str))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
