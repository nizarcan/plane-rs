#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""The Phase 8 cutover exit check (`plans/CUTOVER.md` §5).

Two paths, one script:

* **fresh** — an empty database, no Python image in the stack. `plane-migrator`
  applies the Django baseline, then a scripted API walk signs in, creates a
  workspace, a project, a work item, a comment, uploads an asset, exports the
  project, invites a member (SMTP sink) and delivers a webhook (local
  listener). `/static/logos/github_32px.png` is served by the api and no
  container runs a Python process.
* **upgrade** — a `pg_dump` of a database migrated by Django at the pinned
  migrations is restored; only Rust containers start, `plane-migrator` reports
  "baseline matched", and the same walk runs.

Plus the static check: no deployment file mentions `apps/api/bin`.

    "$(apps/server/tools/upstream/oracle.sh python)" apps/server/tools/cutover/exit_check.py all

The stack is defined in `compose.yml`: project `plane-cutover`, containers
named `plane-cutover-*`, host ports 18500-18519 only, and no frontend images.
The SMTP sink, webhook listener and fake GitHub are one Rust container
(`sink/`), so the "no Python process in any container" assertion is exact.
The compose project is torn down at the end on failure too, unless `--keep`
leaves the stack for inspection and prints the run directory (the images and
the run directory stay after a failure, so a retry does not rebuild and the
logs are still there). The script refuses up front when the Docker daemon
cannot see the checkout (Colima shares only `$HOME` by default), because the
proxy's `Caddyfile.ce` bind mount would otherwise fail with an unattributed
"stack run completed".

`invite` and `webhook` depend on set B's tasks (workspace_invitation and the
model_activity -> webhook_activity -> webhook_send_task chain). The script
reads `RUST_TASKS` from `crates/plane-jobs/src/ownership.rs`: when a task is
not Rust-owned it prints SKIP for that step and continues, because a Celery
publish without a broker is a 500 by design, not a deployment failure. Once
the task is in `RUST_TASKS` the same step is required, so the script becomes
strict without an edit.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

import requests

HERE = Path(__file__).resolve().parent
SERVER = HERE.parent.parent
REPO = SERVER.parent.parent

# The upgrade path restores a database migrated by Django at the pinned commit;
# that Django is the oracle (apps/api at the UPSTREAM.toml pin), not this tree.
sys.path.insert(0, str(SERVER / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402
from oracle import python as _oracle_python  # noqa: E402

API = _oracle_api_dir()
DJANGO = _oracle_python()

PROJECT = "plane-cutover"
COMPOSE = HERE / "compose.yml"
RUN = HERE / "run"
ENV_FILE = RUN / "cutover.env"
DUMP_FILE = RUN / "upgrade.dump"

API_IMAGE = "plane-cutover-api:local"
WORKER_IMAGE = "plane-cutover-worker:local"
BEAT_IMAGE = "plane-cutover-beat:local"
MIGRATOR_IMAGE = "plane-cutover-migrator:local"
SINK_IMAGE = "plane-cutover-sink:local"
IMAGES = [API_IMAGE, WORKER_IMAGE, BEAT_IMAGE, MIGRATOR_IMAGE, SINK_IMAGE]

DB_CONTAINER = "plane-cutover-db"
MIGRATOR_CONTAINER = "plane-cutover-migrator"
API_CONTAINER = "plane-cutover-api"
SINK_CONTAINER = "plane-cutover-sink"
CONTAINERS = [
    DB_CONTAINER,
    "plane-cutover-redis",
    "plane-cutover-minio",
    SINK_CONTAINER,
    MIGRATOR_CONTAINER,
    API_CONTAINER,
    "plane-cutover-worker",
    "plane-cutover-beat",
    "plane-cutover-proxy",
]

BASE_URL = "http://127.0.0.1:18505"
DJANGO_DB_URL = "postgresql://plane:plane@127.0.0.1:18500"
DJANGO_REDIS_URL = "redis://127.0.0.1:18501/0"

INVITE_TASK = "plane.bgtasks.workspace_invitation_task.workspace_invitation"
WEBHOOK_TASKS = [
    "plane.bgtasks.webhook_task.model_activity",
    "plane.bgtasks.webhook_task.webhook_activity",
    "plane.bgtasks.webhook_task.webhook_send_task",
]
EXPORT_TASK = "plane.bgtasks.export_task.issue_export_task"
METADATA_TASK = "plane.bgtasks.storage_metadata_task.get_asset_object_metadata"

EMAIL = "cutover-owner@example.test"
PASSWORD = "Cutover-Passw0rd!2026"
INVITEE = "cutover-invitee@example.test"
WEBHOOK_URL = "http://cutover-sink.local:8080/hook"

PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4"
    "890000000d49444154789c6360000002000154a24f5f0000000049454e44ae42"
    "6082"
)


class Report:
    def __init__(self) -> None:
        self.passed: list[str] = []
        self.failed: list[str] = []
        self.skipped: list[str] = []

    def check(self, label: str, condition: bool, detail: str = "") -> bool:
        if condition:
            print(f"PASS: {label}")
            self.passed.append(label)
            return True
        print(f"FAIL: {label}" + (f"\n      {detail}" if detail else ""))
        self.failed.append(label)
        return False

    def skip(self, label: str, reason: str) -> None:
        print(f"SKIP: {label} ({reason})")
        self.skipped.append(label)

    def summary(self) -> int:
        print()
        print(
            f"summary: {len(self.passed)} passed, {len(self.failed)} failed, "
            f"{len(self.skipped)} skipped"
        )
        if self.skipped:
            print("skipped:")
            for label in self.skipped:
                print(f"  - {label}")
        return 1 if self.failed else 0


REPORT = Report()


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    input_bytes: bytes | None = None,
    timeout: int = 900,
    check: bool = True,
) -> subprocess.CompletedProcess:
    result = subprocess.run(
        command,
        cwd=cwd or REPO,
        env=env,
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"{result.stdout.decode(errors='replace')[-4000:]}"
        )
    return result


def compose(*args: str, check: bool = True, timeout: int = 900) -> subprocess.CompletedProcess:
    return run(
        ["docker", "compose", "-p", PROJECT, "-f", str(COMPOSE), *args],
        check=check,
        timeout=timeout,
    )


def docker(*args: str, check: bool = True, timeout: int = 300) -> subprocess.CompletedProcess:
    return run(["docker", *args], check=check, timeout=timeout)


def psql(sql: str) -> str:
    result = docker(
        "exec", DB_CONTAINER, "psql", "-U", "plane", "-d", "plane", "-tAc", sql
    )
    return result.stdout.decode().strip()


def psql_admin(sql: str) -> str:
    result = docker(
        "exec", DB_CONTAINER, "psql", "-U", "plane", "-d", "postgres", "-tAc", sql
    )
    return result.stdout.decode().strip()


def container_state(name: str) -> tuple[str, int]:
    result = docker(
        "inspect", "-f", "{{.State.Status}} {{.State.ExitCode}}", name, check=False
    )
    if result.returncode != 0:
        return ("missing", -1)
    status, _, code = result.stdout.decode().strip().partition(" ")
    return (status, int(code or "-1"))


def wait_until(label: str, predicate, timeout: int = 180, interval: float = 1.0) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            if predicate():
                return True
        except Exception:
            pass
        time.sleep(interval)
    print(f"      timed out after {timeout}s waiting for {label}")
    return False


# ---------------------------------------------------------------------------
# Ownership (set B dependency)
# ---------------------------------------------------------------------------


def rust_tasks() -> set[str]:
    source = (SERVER / "crates" / "plane-jobs" / "src" / "ownership.rs").read_text()
    block = source.split("pub const RUST_TASKS", 1)[1].split("];", 1)[0]
    return set(re.findall(r'"([^"]+)"', block))


def owned(tasks: set[str], names: list[str]) -> bool:
    return all(name in tasks for name in names)


# ---------------------------------------------------------------------------
# Environment and images
# ---------------------------------------------------------------------------


def vm_available_gib() -> float:
    # `postgres:15.7-alpine` is a stack image, so the probe does not need a
    # pull of its own.
    result = docker("run", "--rm", "postgres:15.7-alpine", "df", "-Pk", "/")
    # ... Filesystem 1024-blocks Used Available Capacity Mounted
    line = result.stdout.decode().strip().splitlines()[-1].split()
    return int(line[3]) / 1024 / 1024


def check_repository_visible() -> None:
    """Refuse when the Docker VM cannot bind-mount the checkout.

    The stack mounts `apps/proxy/Caddyfile.ce` (and the sink's build context)
    from the working tree. Colima shares only `$HOME` by default, so a run from
    `/tmp` made `docker compose up` fail on the proxy's Caddyfile mount and the
    check only reported `FAIL: stack run completed`. Ask the daemon with the
    same bind mount when a local image can answer; otherwise fall back to
    Colima's default share.
    """
    repo = REPO.resolve()
    visible: bool | None = None
    probe_image = "postgres:15.7-alpine"
    if docker("image", "inspect", probe_image, check=False).returncode == 0:
        probe = docker(
            "run",
            "--rm",
            "-v",
            f"{repo}:/cutover-check:ro",
            probe_image,
            "test",
            "-f",
            "/cutover-check/apps/server/tools/cutover/compose.yml",
            check=False,
        )
        if probe.returncode == 0:
            visible = True
        elif probe.returncode == 1:
            visible = False
    if visible is None:
        visible = repo.is_relative_to(Path.home().resolve())
    if not visible:
        raise SystemExit(
            f"refusing to run: the Docker daemon cannot see {repo}.\n"
            "The plane-cutover stack bind-mounts apps/proxy/Caddyfile.ce from the\n"
            "checkout. Colima shares only $HOME by default, so move the checkout\n"
            "under your home directory or share this path with the VM\n"
            "(colima start --mount <path>:w) and retry."
        )


def disk_floor() -> None:
    host = shutil.disk_usage("/")
    print(f"host disk: {host.free / 1024**3:.1f} GiB free")
    if host.free / 1024**3 < 20:
        raise SystemExit("refusing to build: host disk below the 20 GiB floor")
    vm = vm_available_gib()
    print(f"docker VM disk: {vm:.1f} GiB free")
    if vm < 20:
        raise SystemExit("refusing to build: Docker VM disk below the 20 GiB floor")


def build_images() -> None:
    disk_floor()
    print("building plane-api ...")
    run(
        [
            "docker",
            "build",
            "--build-arg",
            "CARGO_BUILD_JOBS=4",
            "-f",
            "apps/server/deploy/docker/Dockerfile.plane-api",
            "-t",
            API_IMAGE,
            ".",
        ],
        timeout=3600,
    )
    for target, image in [
        ("worker", WORKER_IMAGE),
        ("beat", BEAT_IMAGE),
        ("migrator", MIGRATOR_IMAGE),
    ]:
        print(f"building {image} ...")
        run(
            [
                "docker",
                "build",
                "--build-arg",
                "CARGO_BUILD_JOBS=4",
                "-f",
                "apps/server/deploy/docker/Dockerfile.plane-jobs",
                "--target",
                target,
                "-t",
                image,
                ".",
            ],
            timeout=3600,
        )
    print("building cutover-sink ...")
    compose("build", "cutover-sink", timeout=900)


def env_values(allow_celery_dispatch: bool = False) -> dict[str, str]:
    values = {
        "POSTGRES_USER": "plane",
        "POSTGRES_PASSWORD": "plane",
        "POSTGRES_DB": "plane",
        "DATABASE_URL": "postgresql://plane:plane@plane-db:5432/plane",
        "REDIS_URL": "redis://plane-redis:6379/0",
        "SECRET_KEY": secrets.token_hex(32),
        "WEB_URL": BASE_URL,
        "CORS_ALLOWED_ORIGINS": BASE_URL,
        "ALLOWED_HOSTS": "*",
        "DEBUG": "0",
        "ENABLE_SIGNUP": "1",
        "ENABLE_EMAIL_PASSWORD": "1",
        "ENABLE_SMTP": "1",
        "EMAIL_HOST": "cutover-sink",
        "EMAIL_PORT": "25",
        "EMAIL_HOST_USER": "",
        "EMAIL_HOST_PASSWORD": "",
        "EMAIL_FROM": "Cutover <cutover@example.test>",
        "EMAIL_USE_TLS": "0",
        "EMAIL_USE_SSL": "0",
        "USE_MINIO": "1",
        "AWS_REGION": "",
        "AWS_ACCESS_KEY_ID": "access-key",
        "AWS_SECRET_ACCESS_KEY": "secret-key",
        "AWS_S3_ENDPOINT_URL": "http://plane-minio:9000",
        "AWS_S3_BUCKET_NAME": "uploads",
        "FILE_SIZE_LIMIT": "5242880",
        "APP_VERSION": "cutover-test",
        "PLANE_RS_GITHUB_RELEASES_URL": (
            "http://cutover-sink:8080/repos/makeplane/plane/releases/latest"
        ),
        "WEBHOOK_ALLOWED_HOSTS": "cutover-sink.local,cutover-sink",
        "WEBHOOK_ALLOWED_IPS": "",
    }
    # Set B is in flight. Without a broker, a request whose handler publishes a
    # Celery-owned task answers 500 (Django does the same when `.delay()`
    # fails), which would stop the walk at project creation. The test-only
    # ownership override queues those two tasks instead, so the HTTP contract
    # is exercised; the worker has no handler, so the effect is a dead job and
    # the invite/webhook checks stay SKIP. Once the tasks are in RUST_TASKS the
    # override is empty and the steps become strict. Never set this in a
    # deployment (ENV.md §6).
    if allow_celery_dispatch:
        missing = [
            task
            for task in [
                "plane.bgtasks.webhook_task.model_activity",
                INVITE_TASK,
            ]
            if task not in rust_tasks()
        ]
        if missing:
            values["PLANE_RS_JOBS_RUST_TASKS"] = ",".join(missing)
            print(
                "NOTE: --allow-celery-dispatch: queueing the set B tasks with "
                f"the test-only PLANE_RS_JOBS_RUST_TASKS override: {', '.join(missing)}"
            )
    return values


def write_env(values: dict[str, str]) -> None:
    RUN.mkdir(parents=True, exist_ok=True)
    ENV_FILE.write_text("".join(f"{key}={value}\n" for key, value in values.items()))
    ENV_FILE.chmod(0o600)


def load_env() -> dict[str, str]:
    values: dict[str, str] = {}
    for line in ENV_FILE.read_text().splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            values[key] = value
    return values


# ---------------------------------------------------------------------------
# Upgrade snapshot: a database migrated by Django at the pin
# ---------------------------------------------------------------------------


def prepare_upgrade() -> None:
    print("upgrade: starting Postgres and Valkey for the Django snapshot ...")
    compose("up", "-d", "--wait", "plane-db", "plane-redis")
    psql_admin('DROP DATABASE IF EXISTS "plane_upgrade_src" WITH (FORCE)')
    psql_admin('CREATE DATABASE "plane_upgrade_src"')
    env = dict(os.environ)
    env.update(load_env())
    env.update(
        {
            "DJANGO_SETTINGS_MODULE": "plane.settings.production",
            "DATABASE_URL": f"{DJANGO_DB_URL}/plane_upgrade_src",
            "REDIS_URL": DJANGO_REDIS_URL,
            "ENABLE_READ_REPLICA": "0",
        }
    )
    print("upgrade: manage.py migrate (host venv, not a container) ...")
    run(
        [str(DJANGO), "manage.py", "migrate", "--noinput"],
        cwd=API,
        env=env,
        timeout=1200,
    )
    print("upgrade: manage.py configure_instance ...")
    run(
        [str(DJANGO), "manage.py", "configure_instance"],
        cwd=API,
        env=env,
        timeout=300,
    )
    print("upgrade: pg_dump ...")
    dump = docker(
        "exec", DB_CONTAINER, "pg_dump", "-U", "plane", "--no-owner",
        "--no-privileges", "plane_upgrade_src",
    )
    DUMP_FILE.write_bytes(dump.stdout)
    psql_admin('DROP DATABASE IF EXISTS "plane_upgrade_src" WITH (FORCE)')
    psql_admin('DROP DATABASE IF EXISTS "plane" WITH (FORCE)')
    psql_admin('CREATE DATABASE "plane"')
    print("upgrade: restoring the dump ...")
    run(
        ["docker", "exec", "-i", DB_CONTAINER, "psql", "-U", "plane", "-d", "plane"],
        input_bytes=dump.stdout,
        timeout=600,
    )
    REPORT.check(
        "upgrade snapshot: django_migrations is exactly the baseline",
        psql(
            "SELECT count(*) FROM django_migrations"
        )
        == str(
            len((SERVER / "deploy" / "schema" / "django_migrations.txt").read_text().splitlines())
        ),
        f"django_migrations rows: {psql('SELECT count(*) FROM django_migrations')}",
    )


# ---------------------------------------------------------------------------
# Stack
# ---------------------------------------------------------------------------


def down(volumes: bool = True) -> None:
    args = ["down", "--remove-orphans"]
    if volumes:
        args.append("-v")
    compose(*args, check=False, timeout=300)


def start_stack() -> None:
    print("starting the Rust-only stack ...")
    compose("up", "-d", "--remove-orphans")
    status, code = ("missing", -1)
    if wait_until(
        "plane-migrator",
        lambda: container_state(MIGRATOR_CONTAINER)[0] == "exited",
        timeout=300,
    ):
        status, code = container_state(MIGRATOR_CONTAINER)
    REPORT.check(
        "migrator exited 0",
        status == "exited" and code == 0,
        f"status={status} exit={code}",
    )
    logs = docker("logs", MIGRATOR_CONTAINER, check=False).stdout.decode(errors="replace")
    (RUN / "migrator.log").write_text(logs)
    print("migrator output:")
    for line in logs.strip().splitlines():
        print(f"  {line}")
    # `/` goes to the web frontend in Caddyfile.ce (which this stack does not
    # run). `/api/instances/` is the first read that requires the database and
    # the boot-seeded instance configuration.
    if not wait_until(
        "GET /api/instances/ through the proxy",
        lambda: requests.get(f"{BASE_URL}/api/instances/", timeout=2).status_code == 200,
        timeout=240,
    ):
        report_container_logs()
        raise RuntimeError("plane-api did not answer through the proxy")
    REPORT.check("plane-api answers GET /api/instances/", True)


def celery_errors() -> list[str]:
    """Every line proving a request or handler still needed the broker.

    At cutover there is no broker, so a `.delay()` of a task that is not in
    `RUST_TASKS` fails the request (Django's 500) or the handler. The walk must
    fail on either, not merely on a missing e-mail: the point is to prove that
    nothing still needs Celery. Every task is Rust-owned from set T on
    (including `push_instance_metrics`; D34 amendment), so the boot step and
    the beat schedule both go through `plane_jobs` and a broker line is always
    a failure.
    """
    patterns = ["celery publish failed", "plane.exception", "amqp: IO error"]
    found: list[str] = []
    for name in [API_CONTAINER, "plane-cutover-worker", "plane-cutover-beat"]:
        logs = docker("logs", name, check=False).stdout.decode(errors="replace")
        for line in logs.splitlines():
            if any(pattern in line for pattern in patterns):
                found.append(f"{name}: {line}")
    return found


def report_container_logs() -> None:
    for name in [API_CONTAINER, "plane-cutover-worker", "plane-cutover-beat"]:
        result = docker("logs", "--tail", "80", name, check=False)
        print(f"--- {name} ---")
        print(result.stdout.decode(errors="replace"))


def assert_stack() -> None:
    result = docker("ps", "-a", "--filter", f"label=com.docker.compose.project={PROJECT}",
                    "--format", "{{.Names}} {{.Image}} {{.Status}}")
    print("containers:")
    for line in result.stdout.decode().strip().splitlines():
        print(f"  {line}")
    REPORT.check(
        "every project container runs a non-Python image",
        all(
            not re.search(r"python|plane-backend|plane-rs-bench-django", line, re.I)
            for line in result.stdout.decode().splitlines()
        ),
        result.stdout.decode(),
    )
    for image, binary in [
        (API_IMAGE, "plane-api"),
        (WORKER_IMAGE, "plane-worker"),
        (BEAT_IMAGE, "plane-beat"),
        (MIGRATOR_IMAGE, "plane-migrator"),
    ]:
        entrypoint = docker(
            "image", "inspect", "-f", "{{json .Config.Entrypoint}}", image
        ).stdout.decode()
        REPORT.check(
            f"{image} runs {binary}",
            binary in entrypoint and "python" not in entrypoint.lower(),
            entrypoint,
        )
    for name in CONTAINERS:
        top = docker("top", name, check=False)
        processes = top.stdout.decode() if top.returncode == 0 else ""
        REPORT.check(
            f"no python process in {name}",
            "python" not in processes.lower(),
            processes,
        )
    for key in ["django_baseline", "schema_version"]:
        REPORT.check(
            f"plane_jobs.meta has {key}",
            psql(f"SELECT count(*) FROM plane_jobs.meta WHERE key = '{key}'") == "1",
        )
    REPORT.check(
        "instance configuration seeded (36 rows)",
        psql("SELECT count(*) FROM instance_configurations") == "36",
        psql("SELECT count(*) FROM instance_configurations"),
    )
    REPORT.check(
        "one instance row",
        psql("SELECT count(*) FROM instances") == "1",
    )


# ---------------------------------------------------------------------------
# The API walk
# ---------------------------------------------------------------------------


class Walk:
    def __init__(self) -> None:
        self.base = BASE_URL
        self.session = requests.Session()
        self.csrf = ""

    def csrf_token(self) -> str:
        response = self.session.get(f"{self.base}/auth/get-csrf-token/", timeout=10)
        response.raise_for_status()
        self.csrf = response.json()["csrf_token"]
        return self.csrf

    def post(self, path: str, **kwargs: Any) -> requests.Response:
        headers = dict(kwargs.pop("headers", {}))
        headers["X-CSRFToken"] = self.csrf
        headers["Referer"] = f"{self.base}/"
        return self.session.post(
            f"{self.base}{path}", headers=headers, allow_redirects=False, timeout=30, **kwargs
        )

    def patch(self, path: str, **kwargs: Any) -> requests.Response:
        headers = dict(kwargs.pop("headers", {}))
        headers["X-CSRFToken"] = self.csrf
        headers["Referer"] = f"{self.base}/"
        return self.session.patch(
            f"{self.base}{path}", headers=headers, allow_redirects=False, timeout=30, **kwargs
        )

    def get(self, path: str) -> requests.Response:
        return self.session.get(f"{self.base}{path}", timeout=30)


def api_error(response: requests.Response) -> str:
    return f"{response.status_code}: {response.text[:600]}"


def walk(report: Report, tasks: set[str], allow_celery_dispatch: bool = False) -> None:
    print("walking the API ...")
    # A fresh install starts with `instances.is_setup_done = false`: auth is
    # closed until the first instance admin signs up
    # (`license/api/views/admin.py:226-270`). That also creates the User the
    # rest of the walk signs in as, so there is no separate /auth/sign-up/.
    admin = Walk()
    admin.csrf_token()
    response = admin.post(
        "/api/instances/admins/sign-up/",
        data={
            "email": EMAIL,
            "password": PASSWORD,
            "first_name": "Cutover",
            "last_name": "Owner",
            "company_name": "Cutover Inc",
            "is_telemetry_enabled": "True",
            "csrfmiddlewaretoken": admin.csrf,
        },
    )
    report.check(
        "instance admin sign-up answers 302", response.status_code == 302, api_error(response)
    )
    report.check(
        "instance admin sign-up sets admin-session-id",
        "admin-session-id" in admin.session.cookies,
    )
    admin_me = admin.get("/api/instances/admins/me/")
    report.check(
        "GET /api/instances/admins/me/ is 200", admin_me.status_code == 200, api_error(admin_me)
    )
    instances = admin.get("/api/instances/")
    report.check("GET /api/instances/ is 200", instances.status_code == 200, api_error(instances))
    if instances.status_code == 200:
        report.check(
            "INSTANCE_CHANGELOG_URL is the image default",
            instances.json().get("config", {}).get("instance_changelog_url")
            == "https://sites.plane.so/pages/691ef037bcfe416a902e48cb55f59891/",
            json.dumps(instances.json().get("config", {}))[:500],
        )

    signin = Walk()
    signin.csrf_token()
    response = signin.post(
        "/auth/sign-in/",
        data={
            "email": EMAIL,
            "password": PASSWORD,
            "csrfmiddlewaretoken": signin.csrf,
        },
    )
    report.check("sign-in answers 302", response.status_code == 302, api_error(response))
    me = signin.get("/api/users/me/")
    report.check("GET /api/users/me/ is 200", me.status_code == 200, api_error(me))
    user = me.json() if me.status_code == 200 else {}

    signin.csrf_token()
    response = signin.post(
        "/api/workspaces/",
        json={
            "name": "Cutover Workspace",
            "slug": f"cutover-{secrets.token_hex(3)}",
            "organization_size": "1-10",
        },
    )
    report.check("create workspace is 201", response.status_code == 201, api_error(response))
    workspace = response.json() if response.status_code == 201 else {}

    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/projects/",
        json={"name": "Cutover Project", "identifier": "CUT"},
    )
    report.check("create project is 201", response.status_code == 201, api_error(response))
    project = response.json() if response.status_code == 201 else {}

    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/projects/{project.get('id')}/issues/",
        json={"name": "Cutover work item"},
    )
    report.check("create work item is 201", response.status_code == 201, api_error(response))
    issue = response.json() if response.status_code == 201 else {}

    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/projects/{project.get('id')}"
        f"/issues/{issue.get('id')}/comments/",
        json={"comment_html": "<p>cutover comment</p>", "comment_json": {"type": "doc"}},
    )
    report.check("create comment is 201", response.status_code == 201, api_error(response))

    # Read the project to enqueue recent_visited_task and prove a Rust-owned
    # job runs end to end even before set B lands.
    response = signin.get(
        f"/api/workspaces/{workspace.get('slug')}/projects/{project.get('id')}/"
    )
    report.check("GET project is 200", response.status_code == 200, api_error(response))

    # Upload: create the FileAsset + presigned POST, upload the bytes to
    # MinIO, then PATCH to mark it uploaded (which enqueues the metadata task).
    upload = signin.post(
        f"/api/assets/v2/workspaces/{workspace.get('slug')}/",
        json={
            "name": "cutover.png",
            "type": "image/png",
            "size": len(PNG),
            "entity_type": "WORKSPACE_LOGO",
            "entity_identifier": workspace.get("id"),
        },
    )
    uploaded = False
    if report.check("create asset is 200", upload.status_code == 200, api_error(upload)):
        payload = upload.json()
        upload_data = payload.get("upload_data") or {}
        url = upload_data.get("url", "")
        fields = upload_data.get("fields", {})
        # The api generated the URL for the compose service name; the walk
        # runs on the host and uses the published MinIO port. SigV4's presigned
        # POST signs the policy, not the host.
        host_url = url.replace("http://plane-minio:9000", "http://127.0.0.1:18502")
        storage = requests.post(
            host_url,
            data=fields,
            files={"file": ("cutover.png", PNG, "image/png")},
            timeout=30,
        )
        uploaded = storage.status_code in (200, 201, 204)
        report.check(
            "presigned upload to MinIO succeeds",
            uploaded,
            f"{storage.status_code}: {storage.text[:300]}",
        )
        confirm = signin.patch(
            f"/api/assets/v2/workspaces/{workspace.get('slug')}/{payload.get('asset_id')}/",
            json={},
        )
        report.check(
            "asset PATCH marks it uploaded (204)", confirm.status_code == 204, api_error(confirm)
        )

    # Export: the Rust-owned issue_export_task writes exporters.url.
    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/export-issues/",
        json={"provider": "csv", "project": [project.get("id")]},
    )
    report.check("create export is 200", response.status_code == 200, api_error(response))
    exported = wait_until(
        "exporters.status=completed",
        lambda: psql(
            "SELECT status FROM exporters ORDER BY created_at DESC LIMIT 1"
        )
        == "completed",
        timeout=120,
    )
    report.check("export task completes (exporters.status=completed)", exported)
    if exported:
        report.check(
            "export has a presigned url",
            psql("SELECT url IS NOT NULL FROM exporters ORDER BY created_at DESC LIMIT 1")
            == "t",
        )
    report.check(
        "issue_export_task job is done in plane_jobs",
        psql(
            f"SELECT count(*) FROM plane_jobs.jobs WHERE task = '{EXPORT_TASK}' "
            "AND state = 'done'"
        )
        != "0",
    )

    # Invite: SMTP sink (set B dependency, see the module docstring).
    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/invitations/",
        json={"emails": [{"email": INVITEE, "role": 15}]},
    )
    invite_owned = INVITE_TASK in tasks
    if invite_owned:
        report.check(
            "invite is 200", response.status_code == 200, api_error(response)
        )
        mailed = wait_until(
            "invite e-mail at the SMTP sink", lambda: sink_count("smtp-*.eml") > 0, timeout=60
        )
        report.check("invite e-mail reaches the SMTP sink", mailed)
    elif allow_celery_dispatch:
        report.skip(
            "invite e-mail",
            f"{INVITE_TASK} is not in RUST_TASKS yet (set B)",
        )
    else:
        report.check(
            "invite is Rust-owned",
            False,
            f"{INVITE_TASK} is not in RUST_TASKS; the endpoint needs Celery",
        )
    report.check(
        "invitation row exists",
        psql(
            "SELECT count(*) FROM workspace_member_invites WHERE email = "
            f"'{INVITEE}'"
        )
        == "1",
    )

    # Webhook: create one, then create a second work item so the activity
    # chain fires. Delivery needs model_activity/webhook_activity/
    # webhook_send_task (set B).
    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/webhooks/",
        json={
            "url": WEBHOOK_URL,
            "project": True,
            "issue": True,
            "issue_comment": True,
            "module": True,
            "cycle": True,
            "is_active": True,
            "version": "v2",
        },
    )
    report.check("create webhook is 201", response.status_code == 201, api_error(response))
    response = signin.post(
        f"/api/workspaces/{workspace.get('slug')}/projects/{project.get('id')}/issues/",
        json={"name": "Cutover webhook trigger"},
    )
    report.check("webhook trigger work item is 201", response.status_code == 201, api_error(response))
    if owned(tasks, WEBHOOK_TASKS):
        delivered = wait_until(
            "webhook at the local listener", lambda: sink_count("webhook-*.http") > 0, timeout=90
        )
        report.check("webhook reaches the local listener", delivered)
    else:
        missing = [task.rsplit(".", 1)[-1] for task in WEBHOOK_TASKS if task not in tasks]
        if allow_celery_dispatch:
            report.skip("webhook delivery", f"not in RUST_TASKS yet: {', '.join(missing)}")
        else:
            report.check(
                "webhook chain is Rust-owned",
                False,
                f"not in RUST_TASKS: {', '.join(missing)}",
            )

    # /static/* is the deliberate AIO fix and the api's embedded WhiteNoise
    # replacement.
    static = signin.get("/static/logos/github_32px.png")
    expected = (API / "plane" / "static" / "logos" / "github_32px.png").read_bytes()
    report.check(
        "GET /static/logos/github_32px.png is 200 image/png",
        static.status_code == 200 and static.headers.get("Content-Type") == "image/png",
        f"{static.status_code} {static.headers.get('Content-Type')}",
    )
    report.check("static bytes match apps/api/plane/static", static.content == expected)

    print(f"walk completed: workspace={workspace.get('slug')} user={user.get('email')}")


def sink_count(pattern: str) -> int:
    # `docker cp` is daemon-side and works on distroless (no shell).
    target = RUN / "sink"
    if target.exists():
        shutil.rmtree(target)
    target.mkdir(parents=True)
    run(["docker", "cp", f"{SINK_CONTAINER}:/logs/.", str(target)], check=False)
    return len(list(target.glob(pattern)))


# ---------------------------------------------------------------------------
# Static deployment checks
# ---------------------------------------------------------------------------


def static_checks(report: Report) -> None:
    result = run(
        ["git", "grep", "-l", "apps/api/bin", "--", "docker-compose.yml", "deployments", "apps/proxy"],
        check=False,
    )
    report.check(
        "no deployment file mentions apps/api/bin",
        result.returncode != 0 or not result.stdout.strip(),
        result.stdout.decode(),
    )
    compose_config = compose("config", "--format", "json").stdout.decode()
    report.check(
        "compose config has no RabbitMQ service",
        "plane-mq" not in compose_config and "rabbitmq" not in compose_config.lower(),
    )
    report.check(
        "compose config has the migrator completion dependency",
        "service_completed_successfully" in compose_config,
    )


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------


def check_celery_errors(allow_celery_dispatch: bool) -> None:
    errors = celery_errors()
    if allow_celery_dispatch:
        if errors:
            print(
                f"NOTE: --allow-celery-dispatch: {len(errors)} Celery/handler "
                "error line(s) ignored; the cutover gate would fail on them"
            )
        return
    REPORT.check(
        "no Celery dispatch or handler errors in any container",
        not errors,
        "\n".join(errors[:20]),
    )


def cleanup(*, remove_images: bool = True, remove_run: bool = True) -> None:
    down(volumes=True)
    if remove_images:
        for image in IMAGES:
            docker("image", "rm", "-f", image, check=False)
    # `run/` holds the generated SECRET_KEY, the upgrade dump and the sink
    # logs. A green run removes it; a failure keeps it for the report.
    if remove_run:
        shutil.rmtree(RUN, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "mode",
        nargs="?",
        default="all",
        choices=["all", "fresh", "upgrade", "build", "static"],
    )
    parser.add_argument("--keep", action="store_true", help="leave the stack running")
    parser.add_argument(
        "--allow-celery-dispatch",
        action="store_true",
        help=(
            "pre-set-B only: queue the missing e-mail/webhook tasks with the "
            "test-only PLANE_RS_JOBS_RUST_TASKS override and do not fail on "
            "Celery dispatch errors. The default is the cutover gate."
        ),
    )
    args = parser.parse_args()

    RUN.mkdir(parents=True, exist_ok=True)
    values = env_values(args.allow_celery_dispatch)
    write_env(values)
    print(f"run directory: {RUN}")

    if args.mode == "static":
        static_checks(REPORT)
        return REPORT.summary()

    check_repository_visible()

    if args.mode in ("all", "fresh", "upgrade", "build"):
        build_images()

    if args.mode == "build":
        return REPORT.summary()

    tasks = rust_tasks()
    print(f"Rust-owned tasks: {len(tasks)}")

    try:
        if args.mode in ("all", "fresh"):
            print("=" * 72)
            print("FRESH INSTALL")
            print("=" * 72)
            down(volumes=True)
            start_stack()
            REPORT.check(
                "fresh migrator applied the baseline",
                "fresh install: applied the Django baseline" in (RUN / "migrator.log").read_text(),
                (RUN / "migrator.log").read_text()[-2000:],
            )
            assert_stack()
            static_checks(REPORT)
            walk(REPORT, tasks, args.allow_celery_dispatch)
            check_celery_errors(args.allow_celery_dispatch)

        if args.mode in ("all", "upgrade"):
            print("=" * 72)
            print("UPGRADE FROM A DJANGO-MIGRATED DATABASE")
            print("=" * 72)
            down(volumes=True)
            # A fresh env file per path keeps the two databases independent.
            values = env_values(args.allow_celery_dispatch)
            write_env(values)
            prepare_upgrade()
            start_stack()
            REPORT.check(
                "upgrade migrator reports baseline matched",
                "baseline matched" in (RUN / "migrator.log").read_text(),
                (RUN / "migrator.log").read_text()[-2000:],
            )
            assert_stack()
            walk(REPORT, tasks, args.allow_celery_dispatch)
            check_celery_errors(args.allow_celery_dispatch)
    except Exception as error:  # noqa: BLE001 - the exit check reports, then fails
        REPORT.check("stack run completed", False, str(error))
        report_container_logs()
    finally:
        if not args.keep:
            # The compose project is torn down even after a failure: a failed
            # run used to leave `plane-cutover` running while the report said
            # only "stack run completed". The images and `run/` stay on a
            # failure so a retry is cheap and the logs survive.
            cleanup(remove_images=not REPORT.failed, remove_run=not REPORT.failed)

    return REPORT.summary()


if __name__ == "__main__":
    raise SystemExit(main())
