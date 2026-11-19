#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""A local stand-in for GitHub's `releases/latest`, for offline tests.

`register_instance` asks `https://api.github.com/repos/makeplane/plane/releases/latest`
for `tag_name` (`plane/license/management/commands/register_instance.py:40-51`).
Neither the differential harness nor `tools/boot/compare_boot.py` may reach the
internet, so both point `PLANE_RS_GITHUB_RELEASES_URL` at this server (the
Django side is patched by `run_django_boot.py` instead).

    tools/boot/fake_github.py --port 18420 [--tag v9.9.9]
    curl http://127.0.0.1:18420/repos/makeplane/plane/releases/latest
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    tag = "v9.9.9"

    def do_GET(self):  # noqa: N802 - BaseHTTPRequestHandler's naming
        body = json.dumps({"tag_name": self.tag, "name": self.tag}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18420)
    parser.add_argument("--tag", default="v9.9.9")
    args = parser.parse_args()
    Handler.tag = args.tag
    server = ThreadingHTTPServer((args.bind, args.port), Handler)
    print(f"fake-github listening on http://{args.bind}:{args.port}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
