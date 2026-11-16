These files are generated. Do not edit them by hand.

* `ssrf_urls.jsonl` -- 2,400 cases. Each row is one URL plus the frozen DNS
  answer for its hostname, the `WEBHOOK_ALLOWED_IPS` / `WEBHOOK_ALLOWED_HOSTS` /
  `WEBHOOK_DISALLOWED_DOMAINS` inputs, and Python's verdicts for
  `_split_target`, `validate_url`, the pinning resolution (addresses + `Host`
  header) and `WebhookSerializer._validate_webhook_url`.
* `ssrf_ips.jsonl` -- 6,835 address-string cases with every CPython
  `ipaddress` property and `is_blocked_ip`'s answer.

Both are produced by `apps/server/tools/net/ssrf_corpus.py` from the real code
in `apps/api/plane/utils/{url_security,ip_address}.py` and
`apps/api/plane/app/serializers/webhook.py`, run on the repository's virtualenv
(CPython 3.12.13 at the time of writing) with `socket.getaddrinfo` stubbed so
DNS is deterministic on both sides.

Regenerate after an upstream change to those files, and commit the result:

    apps/api/.venv/bin/python apps/server/tools/net/ssrf_corpus.py
    apps/api/.venv/bin/python apps/server/tools/net/ssrf_corpus.py --check

`--check` exits non-zero when the committed corpus is stale; run it in CI
wherever the Django virtualenv is available. A failing Rust test means the port
differs from Django -- fix the port. Regenerating is only correct when the
Python oracle itself changed, and the diff then has to be read.
