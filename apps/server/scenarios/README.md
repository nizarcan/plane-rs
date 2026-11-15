# Differential scenarios

`difftest` replays a recorded request session against two targets and compares
every observable side effect. It is the primary verification tool for the Rust
rewrite: a ported route is not done until the same scenario produces the same
status, headers, bodies, database diff and Celery messages on Django and on
`plane-api`.

```bash
cd apps/server
cargo run -p plane-testkit --bin difftest -- selftest
cargo run -p plane-testkit --bin difftest -- \
  run --scenario scenarios/basic-session.yaml --a django --b django
```

Exit code is 0 only when every step matches. `--report report.json` writes the
full run (normalised bodies, headers, database diffs, Celery messages and the
normalisation rules that applied).

## Files

| File | Purpose |
| --- | --- |
| `targets.yaml` | How to start each kind of target (`django` today, `rust` later) |
| `normalize.yaml` | The complete list of legitimately nondeterministic values, each with a justification |
| `basic-session.yaml` | First-run web-app session: instance setup, sign-up, workspace, project, work items, comments, sign-out, error paths |

## What a run does

1. Allocates a run id and clones `plane_tpl` into `dt_<run>_a` and
   `dt_<run>_b` (unless `--a-db`/`--b-db` attach to existing databases).
2. Flushes the two Redis DB indexes (`redis_db: [1, 2]` in the scenario by
   default) and creates the RabbitMQ vhosts `dt_<run>_a` / `_b` with full
   permissions for `plane`.
3. Starts both targets on free ports from their `port_range`, waits for the
   health path, then replays the steps in order with both targets advancing
   together.
4. For each step: snapshot the database before and after, send the request,
   drain every queue in the target's vhost, normalise, compare, and print
   `ok` / `DIFF` with a unified diff per difference.
5. Stops the targets by killing their process group (gunicorn workers
   included) and drops the databases and vhosts unless `--keep`.

## Scenario format

```yaml
name: example
template_db: plane_tpl          # default
vars:                           # scenario variables, available as {{name}}
  email: user@example.com
redis_db: [1, 2]                # Redis DB indexes for A and B
steps:
  - name: sign-in
    method: POST
    path: /auth/sign-in/
    jar: user                   # named cookie jar, default `default`
    form:                       # urlencoded body
      email: "{{email}}"
      csrfmiddlewaretoken: "{{csrf}}"
    headers:
      Origin: "{{base_url}}"    # implicit per-target variable
    expect_status: 302          # sanity check applied to both targets
    capture:
      - name: csrf
        source: json            # json | header | cookie | regex
        pointer: /csrf_token
      - name: location
        source: regex
        header: Location
        pattern: '/workspaces/([^/]+)/'
```

`json:` sends a JSON body, `body:` sends a raw string body (default
`text/plain;charset=UTF-8`). Variables are per target: A captures and uses
A's values, B captures and uses B's. `capture` supports a JSON pointer, a
whole header, a cookie from the step's jar, or the first group of a regex over
a header (useful for `Location`).

Every request sends `Accept-Encoding: gzip` and a fixed browser
`User-Agent`. Redirects are not followed, cookies are not stored implicitly,
and gzip bodies are decoded by the harness.

## Normalisation policy

Only values that are genuinely random or clock-based are rewritten. The
table lives in `normalize.yaml`; every entry has a `why` that is copied into
the report. The built-in rules are:

- generated UUIDs (not present in the baseline database) become `<id:N>`,
  numbered per target in the fixed order headers (`Location` first) → body in
  document order → database diff by table and row → Celery messages;
- ISO-8601 timestamps inside the step window become `<now>`; a timestamp the
  target wrote in an earlier step stays `<now>` if it resurfaces;
- the target's own base URL becomes `<base>`;
- cookie `Expires` becomes `<expires+Ns>` and database timestamps that are a
  fixed offset from the step clock become `<offset±Ns>`;
- Django's randomized gzip filename makes `Content-Encoding`/`Content-Length`
  unstable near the 200-byte threshold, so that decision is normalised; the
  decoded body is always compared;
- Celery's kombu `uuid`/`datetime` envelopes are unwrapped for normalisation.

If a Django-vs-Django run reports a new difference, find the cause before
adding a rule. A rule that hides a real difference weakens every future port.

## Comparing a difference

For each step the harness compares:

1. status code;
2. all headers except `date`, `server` and (when unstable) `content-length`,
   case-insensitively, with multi-valued headers as ordered lists;
   `Set-Cookie` by cookie name and attributes, with volatile cookie values
   replaced;
3. body: JSON parsed with key order and number lexemes kept, plus D15's byte
   rule (each side must equal its own DRF-compact re-serialisation);
4. the database diff (inserted/deleted/updated rows per table, as multisets);
5. drained Celery messages (task name, args, kwargs and non-volatile headers).

## Attach mode

To compare targets that are already running (for debugging, or to inject a
divergence into one database):

```bash
PLANE_DB=t_a PLANE_REDIS_DB=1 PLANE_MQ_VHOST=t_a dev/django-serve.sh 18001 &
PLANE_DB=t_b PLANE_REDIS_DB=2 PLANE_MQ_VHOST=t_b dev/django-serve.sh 18002 &

difftest run --scenario scenarios/basic-session.yaml \
  --a-url http://127.0.0.1:18001 --b-url http://127.0.0.1:18002 \
  --a-db t_a --b-db t_b --a-vhost t_a --b-vhost t_b
```

Attach mode does not clone or drop databases or vhosts, and the harness does
not stop the targets. `--no-celery` skips message draining when the vhosts are
not known.

`--a-var name=value` / `--b-var name=value` override a scenario variable for
one target. That is the easiest way to prove the harness detects a difference:
for example `--b-var company_role=Saboteur` changes only a database column
that no response echoes, so a clean run must turn into a database-only DIFF.

## Debugging

```bash
# Snapshot timing and row counts for a clone (not the template: it is
# ALLOW_CONNECTIONS false)
dev/mkdb.sh t_inspect
difftest snapshot --db t_inspect --iterations 5
dev/dropdb.sh t_inspect
```

Target logs are written to
`apps/server/target/difftest-logs/<run>/a.log` and `b.log` (gitignored). `--keep` also keeps the databases and vhosts for
inspection; drop them with `dev/dropdb.sh` and `dev/mq-vhost.sh --delete`.
