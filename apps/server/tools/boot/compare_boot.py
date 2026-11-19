#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Compare the Rust boot steps and operator commands with Django's.

For each step in `apps/api/bin/docker-entrypoint-api.sh:3-33` and each command in
`plans/CUTOVER.md` §3, run the Django command on clone A and `plane-admin` (or
`plane-api`'s boot function) on clone B, then compare the observable effects:
rows, Redis keys, the bucket, the SMTP transcript and the command output.

    apps/api/.venv/bin/python tools/boot/compare_boot.py

Uses only `t_p8boot_boot_a` / `t_p8boot_boot_b`, Redis DBs 7 and 8, the bucket
`p8boot-boot`, the fake GitHub on 18420 and one `fake_smtp.py` listener at a
time on an ephemeral port (`--port 0 --port-file`, as `difftest task` does, so a
concurrent run's SMTP wrapper cannot collide with it). Drops the databases and
bucket and kills every child process it started on exit (PIDs are recorded as
they are spawned).

Two documented differences are asserted rather than hidden:

* `register_instance` counts one `push_instance_metrics.delay()` on A and zero
  on B (owner decision 5: not ported).
* Python's `getpass` writes a `GetPassWarning` block to stderr when stdin is a
  pipe and Rust's prompt does not; the meaningful stderr lines are still
  compared byte for byte.
"""

import argparse
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import boto3
import psycopg
import redis
from botocore.exceptions import ClientError

SERVER_DIR = Path(__file__).resolve().parent.parent.parent
REPO_ROOT = SERVER_DIR.parent.parent

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402
from oracle import python as _oracle_python  # noqa: E402

API_DIR = _oracle_api_dir()
API_PYTHON = _oracle_python()
ADMIN = SERVER_DIR / "target" / "debug" / "plane-admin"
TOOLS = SERVER_DIR / "tools"
GITHUB = TOOLS / "boot" / "fake_github.py"
FAKE_SMTP = TOOLS / "mail" / "fake_smtp.py"
COMPARE_SMTP = TOOLS / "mail" / "compare_smtp_logs.py"

# The defaults are the original single-agent names. Concurrent agents override
# every one of them (`P8BOOT_A_DB`, `P8BOOT_B_DB`, `P8BOOT_A_REDIS`,
# `P8BOOT_B_REDIS`, `P8BOOT_BUCKET`, `P8BOOT_GITHUB_PORT`), because the boot
# check creates and drops databases by fixed name, flushes Redis DBs and binds
# ports; two runs sharing them terminate each other's connections.
A_DB = os.environ.get("P8BOOT_A_DB", "t_p8boot_boot_a")
B_DB = os.environ.get("P8BOOT_B_DB", "t_p8boot_boot_b")
A_REDIS = int(os.environ.get("P8BOOT_A_REDIS", "7"))
B_REDIS = int(os.environ.get("P8BOOT_B_REDIS", "8"))
BUCKET = os.environ.get("P8BOOT_BUCKET", "p8boot-boot")
GITHUB_PORT = int(os.environ.get("P8BOOT_GITHUB_PORT", "18420"))
SMTP_LOG_A = Path(f"/tmp/p8boot-smtp-a-{A_DB}.jsonl")
SMTP_LOG_B = Path(f"/tmp/p8boot-smtp-b-{B_DB}.jsonl")
SMTP_PORT_A = Path(f"/tmp/p8boot-smtp-a-{A_DB}.port")
SMTP_PORT_B = Path(f"/tmp/p8boot-smtp-b-{B_DB}.port")

USER_ID = "6f1e7a52-2f5e-4c4a-9d38-2c0e2a9b6a11"
USER_EMAIL = "p8boot-user@example.test"
OLD_PASSWORD = "Old-p8boot-password-1!"
STRONG_PASSWORD = "Tr0ub4dor&3xY9!qZ"

PIDS: list[int] = []
FAILURES: list[str] = []


def check(label: str, condition: bool, detail: str = "") -> bool:
    if condition:
        print(f"PASS: {label}")
        return True
    FAILURES.append(label)
    print(f"FAIL: {label}" + (f"\n{detail}" if detail else ""))
    return False


# ---------------------------------------------------------------------------
# Process and environment helpers
# ---------------------------------------------------------------------------


def spawn(command, *, env=None, stdout=None, stderr=None) -> subprocess.Popen:
    process = subprocess.Popen(command, env=env, stdout=stdout, stderr=stderr)
    PIDS.append(process.pid)
    return process


def stop_process(process: subprocess.Popen) -> None:
    if process.poll() is None:
        process.terminate()
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    if process.pid in PIDS:
        PIDS.remove(process.pid)


def start_smtp(log: Path, port_file: Path) -> tuple[subprocess.Popen, int]:
    """Start a listener on an ephemeral port and return (process, port)."""
    log.write_text("", encoding="utf-8")
    port_file.unlink(missing_ok=True)
    process = spawn(
        [
            sys.executable,
            str(FAKE_SMTP),
            "--port",
            "0",
            "--port-file",
            str(port_file),
            "--output",
            str(log),
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    deadline = time.monotonic() + 10
    port: int | None = None
    while time.monotonic() < deadline:
        if port_file.exists():
            text = port_file.read_text(encoding="utf-8").strip()
            if text:
                port = int(text)
                break
        time.sleep(0.05)
    if port is None:
        raise SystemExit("fake SMTP did not write its port file")
    while time.monotonic() < deadline:
        probe = run(["bash", "-c", f"exec 3<>/dev/tcp/127.0.0.1/{port}"], env=os.environ)
        if probe.returncode == 0:
            # The probe is itself a greeting-only SMTP transaction; drop it so
            # the log holds only the command's message.
            time.sleep(0.2)
            log.write_text("", encoding="utf-8")
            return process, port
        time.sleep(0.1)
    raise SystemExit(f"fake SMTP on {port} did not start")


def stop_all() -> None:
    for pid in PIDS:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
    for pid in PIDS:
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            continue
    PIDS.clear()


def dev_env(db: str, redis_db: int, extra: dict | None = None) -> dict:
    """The environment `dev/django-env.sh` exports for one clone."""
    base = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", "/tmp"),
        "PLANE_DB": db,
        "PLANE_REDIS_DB": str(redis_db),
        "PLANE_MQ_VHOST": "p8boot",
    }
    script = f'source "{SERVER_DIR}/dev/django-env.sh"; env -0'
    output = subprocess.check_output(["bash", "-c", script], env=base)
    env = {}
    for entry in output.split(b"\0"):
        if b"=" not in entry:
            continue
        key, _, value = entry.partition(b"=")
        env[key.decode()] = value.decode()
    if extra:
        env.update(extra)
    return env


def run(command, *, env=None, stdin: str | None = None, cwd: str | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(
        command,
        env=env,
        cwd=cwd,
        input=stdin,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def database_url(env: dict) -> str:
    return env["DATABASE_URL"]


def psql(env: dict):
    return psycopg.connect(database_url(env))


# ---------------------------------------------------------------------------
# Database, Redis and S3 setup
# ---------------------------------------------------------------------------


def admin_connection(env: dict):
    url = database_url(env)
    base, _, _ = url.rpartition("/")
    return psycopg.connect(f"{base}/postgres", autocommit=True)


def drop_database(env: dict, name: str) -> None:
    with admin_connection(env) as connection, connection.cursor() as cursor:
        cursor.execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity "
            "WHERE datname = %s AND pid <> pg_backend_pid()",
            (name,),
        )
        cursor.execute(f'DROP DATABASE IF EXISTS "{name}" WITH (FORCE)')


def create_database(env: dict, name: str, template: str = "plane_tpl") -> None:
    drop_database(env, name)
    with admin_connection(env) as connection, connection.cursor() as cursor:
        cursor.execute(f'CREATE DATABASE "{name}" TEMPLATE "{template}"')


def seed(env: dict) -> None:
    """The user the admin commands act on and the SMTP rows test_email reads.

    EMAIL_PORT starts at 0; the test-email step writes each listener's
    ephemeral port into the side that is about to run.
    """
    result = run(
        [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "seed_user",
         USER_ID, USER_EMAIL, OLD_PASSWORD],
        env=env,
    )
    if result.returncode != 0:
        raise SystemExit(f"seeding the user failed:\n{result.stdout}\n{result.stderr}")
    values = {
        "ENABLE_SMTP": ("1", False),
        "EMAIL_HOST": ("127.0.0.1", False),
        "EMAIL_HOST_USER": ("", False),
        "EMAIL_HOST_PASSWORD": ("", True),
        "EMAIL_PORT": ("0", False),
        "EMAIL_FROM": ("Team Plane <team@example.test>", False),
        "EMAIL_USE_TLS": ("0", False),
        "EMAIL_USE_SSL": ("0", False),
    }
    with psql(env) as connection, connection.cursor() as cursor:
        for key, (value, encrypted) in values.items():
            cursor.execute(
                "UPDATE instance_configurations SET value = %s, is_encrypted = %s "
                "WHERE key = %s",
                (value, encrypted, key),
            )
        connection.commit()


def set_email_port(env: dict, port: int) -> None:
    with psql(env) as connection, connection.cursor() as cursor:
        cursor.execute(
            "UPDATE instance_configurations SET value = %s WHERE key = 'EMAIL_PORT'",
            (str(port),),
        )
        connection.commit()


def rows(env: dict, query: str) -> list[tuple]:
    with psql(env) as connection, connection.cursor() as cursor:
        cursor.execute(query)
        return [tuple("" if value is None else str(value) for value in row) for row in cursor.fetchall()]


def encrypt_matches(env: dict, rows_a: list[tuple], rows_b: list[tuple]) -> list[str]:
    """Compare InstanceConfiguration rows; encrypted values by plaintext."""
    from cryptography.fernet import Fernet
    import base64
    import hashlib

    secret = env["SECRET_KEY"]
    key = base64.urlsafe_b64encode(hashlib.pbkdf2_hmac("sha256", secret.encode(), b"salt", 100000))
    fernet = Fernet(key)

    def decrypt(value: str) -> str:
        if not value:
            return ""
        return fernet.decrypt(value.encode()).decode()

    problems = []
    if len(rows_a) != len(rows_b):
        problems.append(f"row count {len(rows_a)} vs {len(rows_b)}")
    for left, right in zip(rows_a, rows_b):
        if (left[0], left[2], left[3]) != (right[0], right[2], right[3]):
            problems.append(f"key/category/encrypted differ: {left} vs {right}")
            continue
        if left[3] == "True":
            # `encrypt_data(None)` is `""` (not a Fernet token), so an empty
            # value on both sides is the expected shape for an unset secret.
            try:
                if decrypt(left[1]) != decrypt(right[1]):
                    problems.append(f"{left[0]}: plaintext differs")
            except Exception as error:  # noqa: BLE001 - reported as a failure
                problems.append(f"{left[0]}: decryption failed: {error}")
        elif left[1] != right[1]:
            problems.append(f"{left[0]}: value {left[1]!r} vs {right[1]!r}")
    return problems


def redis_client(env: dict):
    return redis.Redis.from_url(env["REDIS_URL"])


def s3_client(env: dict):
    return boto3.client(
        "s3",
        endpoint_url=env["AWS_S3_ENDPOINT_URL"],
        aws_access_key_id=env["AWS_ACCESS_KEY_ID"],
        aws_secret_access_key=env["AWS_SECRET_ACCESS_KEY"],
        region_name=env["AWS_REGION"],
        config=boto3.session.Config(signature_version="s3v4"),
    )


def delete_bucket(client, name: str) -> None:
    try:
        objects = client.list_objects_v2(Bucket=name).get("Contents", [])
        for item in objects:
            client.delete_object(Bucket=name, Key=item["Key"])
        client.delete_bucket(Bucket=name)
    except ClientError:
        pass


def bucket_exists(client, name: str) -> bool:
    try:
        client.head_bucket(Bucket=name)
        return True
    except ClientError:
        return False


# ---------------------------------------------------------------------------
# Comparison steps
# ---------------------------------------------------------------------------


def compare_output(label: str, a: subprocess.CompletedProcess, b: subprocess.CompletedProcess) -> None:
    check(
        f"{label}: exit code",
        a.returncode == b.returncode,
        f"A={a.returncode} B={b.returncode}\nA stdout={a.stdout!r}\nB stdout={b.stdout!r}",
    )
    check(
        f"{label}: stdout",
        a.stdout == b.stdout,
        f"A={a.stdout!r}\nB={b.stdout!r}",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    for binary in (ADMIN,):
        if not binary.exists():
            raise SystemExit(f"{binary} does not exist; run `cargo build -p plane-admin`")

    env_a = dev_env(A_DB, A_REDIS, {"AWS_S3_BUCKET_NAME": BUCKET})
    env_b = dev_env(
        B_DB,
        B_REDIS,
        {
            "AWS_S3_BUCKET_NAME": BUCKET,
            "PLANE_RS_GITHUB_RELEASES_URL": f"http://127.0.0.1:{GITHUB_PORT}/repos/makeplane/plane/releases/latest",
        },
    )

    for path in (SMTP_LOG_A, SMTP_LOG_B, SMTP_PORT_A, SMTP_PORT_B):
        path.unlink(missing_ok=True)
    docker = shutil.which("docker")

    try:
        print("== resources")
        create_database(env_a, A_DB)
        create_database(env_b, B_DB)
        seed(env_a)
        seed(env_b)
        s3_a = s3_client(env_a)
        delete_bucket(s3_a, BUCKET)
        redis_client(env_a).flushdb()
        redis_client(env_b).flushdb()

        spawn(
            [sys.executable, str(GITHUB), "--port", str(GITHUB_PORT)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        time.sleep(0.7)
        probe = run(["bash", "-c", f"exec 3<>/dev/tcp/127.0.0.1/{GITHUB_PORT}"], env=os.environ)
        if probe.returncode != 0:
            raise SystemExit(f"port {GITHUB_PORT} is not listening")

        # --- register_instance: create, then extend -------------------------
        print("== register_instance (fresh database)")
        django_register = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "register_instance", "p8boot-signature"],
            env=env_a,
        )
        rust_register = run([str(ADMIN), "register-instance", "p8boot-signature"], env=env_b)
        compare_output("register_instance create", django_register, rust_register)
        check(
            "register_instance A enqueued push_instance_metrics, B did not (decision 5)",
            "PUSH_INSTANCE_METRICS_CALLS=1" in django_register.stderr
            and "PUSH_INSTANCE_METRICS_CALLS" not in rust_register.stderr,
            f"A stderr={django_register.stderr!r}",
        )

        instance_query = (
            "SELECT instance_name, whitelist_emails, current_version, latest_version, "
            "edition, domain, namespace, is_telemetry_enabled, is_support_required, "
            "is_setup_done, is_signup_screen_visited, is_verified, is_test, "
            "is_current_version_deprecated FROM instances"
        )
        rows_a = rows(env_a, instance_query)
        rows_b = rows(env_b, instance_query)
        check("register_instance rows", rows_a == rows_b, f"A={rows_a}\nB={rows_b}")
        check("register_instance latest_version from the fake", rows_b and rows_b[0][3] == "v9.9.9", f"{rows_b}")
        ids_b = rows(env_b, "SELECT instance_id FROM instances")
        check(
            "register_instance instance_id is 24 lowercase hex",
            len(ids_b) == 1 and re.fullmatch(r"[0-9a-f]{24}", ids_b[0][0]) is not None,
            f"{ids_b}",
        )

        django_extend = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "register_instance", "p8boot-signature"],
            env=env_a,
        )
        rust_extend = run([str(ADMIN), "register-instance", "p8boot-signature"], env=env_b)
        compare_output("register_instance extend", django_extend, rust_extend)
        check(
            "register_instance extend rows",
            rows(env_a, instance_query) == rows(env_b, instance_query),
        )

        # --- configure_instance --------------------------------------------
        print("== configure_instance")
        # A deprecated key with no environment override, so the insert path runs
        # identically on both sides. (`EMAIL_PORT` carries a per-side SMTP port
        # and is deliberately not used.)
        for env in (env_a, env_b):
            with psql(env) as connection, connection.cursor() as cursor:
                cursor.execute(
                    "DELETE FROM instance_configurations WHERE key = 'GPT_ENGINE'"
                )
                connection.commit()
        django_configure = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "configure_instance"],
            env=env_a,
        )
        rust_configure = run([str(ADMIN), "configure-instance"], env=env_b)
        compare_output("configure_instance", django_configure, rust_configure)
        config_query = "SELECT key, value, category, is_encrypted FROM instance_configurations ORDER BY key"
        problems = encrypt_matches(env_a, rows(env_a, config_query), rows(env_b, config_query))
        check("configure_instance rows (encrypted values by plaintext)", not problems, "\n".join(problems))
        check(
            "configure_instance 36 rows",
            len(rows(env_b, "SELECT key FROM instance_configurations")) == 36,
        )
        encrypted_a = [row[1] for row in rows(env_a, config_query) if row[3] == "True" and row[1]]
        encrypted_b = [row[1] for row in rows(env_b, config_query) if row[3] == "True" and row[1]]
        check(
            "configure_instance ciphertexts are independently random",
            len(encrypted_a) == len(encrypted_b) and all(a != b for a, b in zip(encrypted_a, encrypted_b)),
        )

        # --- create_bucket --------------------------------------------------
        print("== create_bucket")
        delete_bucket(s3_a, BUCKET)
        check("create_bucket starts absent", not bucket_exists(s3_a, BUCKET))
        # A creates the bucket, then it is deleted again so B takes the same
        # path and the outputs are comparable without masking the name.
        django_bucket = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "create_bucket"],
            env=env_a,
        )
        created_a = bucket_exists(s3_a, BUCKET)
        delete_bucket(s3_a, BUCKET)
        rust_bucket = run([str(ADMIN), "create-bucket"], env=env_b)
        created_b = bucket_exists(s3_a, BUCKET)
        compare_output("create_bucket (missing)", django_bucket, rust_bucket)
        check("create_bucket created on both sides", created_a and created_b)
        django_exists = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "create_bucket"],
            env=env_a,
        )
        rust_exists = run([str(ADMIN), "create-bucket"], env=env_b)
        compare_output("create_bucket (exists)", django_exists, rust_exists)

        # --- clear_cache ----------------------------------------------------
        print("== clear_cache")
        redis_a = redis_client(env_a)
        redis_b = redis_client(env_b)
        for client in (redis_a, redis_b):
            client.set(":1:p8boot-throttle", b"1")
            client.set(":1:p8boot-cache", b"2")
            client.set("p8boot-magic", b"3")
        django_cache = run(
            [str(API_PYTHON), str(TOOLS / "boot" / "run_django_boot.py"), "clear_cache"],
            env=env_a,
        )
        rust_cache = run([str(ADMIN), "clear-cache"], env=env_b)
        compare_output("clear_cache", django_cache, rust_cache)
        check(
            "clear_cache FLUSHDB on both DBs",
            redis_a.dbsize() == 0 and redis_b.dbsize() == 0,
            f"A={redis_a.dbsize()} B={redis_b.dbsize()}",
        )

        # --- reset_password -------------------------------------------------
        print("== reset-password")
        django_reset = run(
            [str(API_PYTHON), "manage.py", "reset_password", USER_EMAIL],
            env=env_a,
            cwd=str(API_DIR),
            stdin="abc\nxyz\n",
        )
        rust_reset = run(
            [str(ADMIN), "reset-password", USER_EMAIL],
            env=env_b,
            stdin="abc\nxyz\n",
        )
        compare_reset_output("reset-password mismatch", django_reset, rust_reset)
        check("reset-password mismatch leaves the hash", not password_changed(env_a, env_b))

        django_weak = run(
            [str(API_PYTHON), "manage.py", "reset_password", USER_EMAIL],
            env=env_a,
            cwd=str(API_DIR),
            stdin="password\npassword\n",
        )
        rust_weak = run(
            [str(ADMIN), "reset-password", USER_EMAIL],
            env=env_b,
            stdin="password\npassword\n",
        )
        compare_reset_output("reset-password weak", django_weak, rust_weak)
        check(
            "reset-password weak is a CommandError on both",
            "CommandError: Password is too common please set a complex password"
            in normalise_getpass_stderr(django_weak.stderr)
            and "CommandError: Password is too common please set a complex password"
            in normalise_getpass_stderr(rust_weak.stderr),
            f"A={django_weak.stderr!r}\nB={rust_weak.stderr!r}",
        )
        check("reset-password weak leaves the hash", not password_changed(env_a, env_b))

        strong_stdin = f"{STRONG_PASSWORD}\n{STRONG_PASSWORD}\n"
        django_strong = run(
            [str(API_PYTHON), "manage.py", "reset_password", USER_EMAIL],
            env=env_a,
            cwd=str(API_DIR),
            stdin=strong_stdin,
        )
        rust_strong = run(
            [str(ADMIN), "reset-password", USER_EMAIL],
            env=env_b,
            stdin=strong_stdin,
        )
        compare_reset_output("reset-password strong", django_strong, rust_strong)
        check(
            "reset-password sets a verifying hash and clears is_password_autoset on both",
            password_matches(env_a, env_b, STRONG_PASSWORD),
        )

        django_missing = run(
            [str(API_PYTHON), "manage.py", "reset_password", "nobody@example.test"],
            env=env_a,
            cwd=str(API_DIR),
            stdin="",
        )
        rust_missing = run(
            [str(ADMIN), "reset-password", "nobody@example.test"],
            env=env_b,
            stdin="",
        )
        compare_reset_output("reset-password missing user", django_missing, rust_missing)

        # --- activate_user --------------------------------------------------
        print("== activate-user")
        django_activate = run(
            [str(API_PYTHON), "manage.py", "activate_user", USER_EMAIL], env=env_a, cwd=str(API_DIR)
        )
        rust_activate = run([str(ADMIN), "activate-user", USER_EMAIL], env=env_b)
        compare_output("activate-user", django_activate, rust_activate)
        check(
            "activate-user is_active true on both",
            rows(env_a, "SELECT is_active FROM users") == rows(env_b, "SELECT is_active FROM users")
            == [("True",)],
        )
        django_activate_missing = run(
            [str(API_PYTHON), "manage.py", "activate_user", "nobody@example.test"], env=env_a, cwd=str(API_DIR)
        )
        rust_activate_missing = run(
            [str(ADMIN), "activate-user", "nobody@example.test"], env=env_b
        )
        compare_output("activate-user missing user", django_activate_missing, rust_activate_missing)
        check(
            "activate-user missing is a CommandError on both",
            "CommandError: Error: User with nobody@example.test does not exists"
            in django_activate_missing.stderr
            and "CommandError: Error: User with nobody@example.test does not exists"
            in rust_activate_missing.stderr,
        )

        # --- test_email -----------------------------------------------------
        print("== test-email")
        # A listener per side on a kernel-chosen port: the fixed 18411 collided
        # with the SMTP scenario wrapper's port once in a full verification
        # run. Only the side under test points its EMAIL_PORT row at its
        # listener, and each log then holds exactly one message.
        smtp_a, port_a = start_smtp(SMTP_LOG_A, SMTP_PORT_A)
        set_email_port(env_a, port_a)
        django_email = run(
            [str(API_PYTHON), "manage.py", "test_email", "receiver@example.test"], env=env_a, cwd=str(API_DIR)
        )
        stop_process(smtp_a)
        smtp_b, port_b = start_smtp(SMTP_LOG_B, SMTP_PORT_B)
        set_email_port(env_b, port_b)
        rust_email = run([str(ADMIN), "test-email", "receiver@example.test"], env=env_b)
        stop_process(smtp_b)
        compare_output("test-email", django_email, rust_email)
        time.sleep(0.5)
        smtp = run(
            [
                str(API_PYTHON),
                str(COMPARE_SMTP),
                "--a",
                str(SMTP_LOG_A),
                "--b",
                str(SMTP_LOG_B),
                "--expect",
                "test-email",
            ]
        )
        print(smtp.stdout.strip())
        check("test-email SMTP transcript", smtp.returncode == 0, smtp.stderr)

        # --- boot step outputs ---------------------------------------------
    finally:
        stop_all()
        if args.keep:
            print(f"kept {A_DB}, {B_DB}, bucket {BUCKET}")
        else:
            for name in (A_DB, B_DB):
                drop_database(env_a, name)
            delete_bucket(s3_client(env_a), BUCKET)
            for path in (SMTP_LOG_A, SMTP_LOG_B, SMTP_PORT_A, SMTP_PORT_B):
                path.unlink(missing_ok=True)

    if FAILURES:
        print(f"\n{len(FAILURES)} failure(s):")
        for failure in FAILURES:
            print(f"  - {failure}")
        return 1
    print("\nPASS: every boot step and operator command matches Django")
    return 0


ANSI = re.compile(r"\x1b\[[0-9;]*m")
# Lines that are the interpreter's or the local stack's, not the command's:
# Python's piped-stdin getpass warning, and the dev stack's placeholder
# object-storage credentials warning (Django prints it as a bare line, Rust via
# tracing with its own format).
NOISE = (
    "GetPassWarning: Can not control echo on the terminal",
    "passwd = fallback_getpass",
    "Warning: Password input may be echoed.",
    "SECURITY: AWS_SECRET_ACCESS_KEY is set to the placeholder value",
    "SECURITY: MinIO is enabled with the public placeholder credentials",
)


def normalise_getpass_stderr(text: str) -> str:
    """The command's own stderr lines, without the two environment artefacts."""
    text = ANSI.sub("", text)
    text = text.replace("Password (again): ", "").replace("Password: ", "")
    kept = [
        line
        for line in text.splitlines()
        if line.strip() and not any(noise in line for noise in NOISE)
    ]
    return "\n".join(kept)


def compare_reset_output(label: str, a: subprocess.CompletedProcess, b: subprocess.CompletedProcess) -> None:
    check(f"{label}: exit code", a.returncode == b.returncode, f"A={a.returncode} B={b.returncode}")
    check(f"{label}: stdout", a.stdout == b.stdout, f"A={a.stdout!r}\nB={b.stdout!r}")
    normalised_a = normalise_getpass_stderr(a.stderr)
    normalised_b = normalise_getpass_stderr(b.stderr)
    check(f"{label}: stderr (piped-getpass warning normalised)", normalised_a == normalised_b,
          f"A={normalised_a!r}\nB={normalised_b!r}")


def password_rows(env: dict):
    return rows(env, f"SELECT password, is_password_autoset FROM users WHERE id = '{USER_ID}'")


def check_password_hash(plaintext: str, encoded: str) -> bool:
    """Django's `pbkdf2_sha256` verification, without importing Django."""
    import base64
    import hashlib

    algorithm, iterations, salt, digest = encoded.split("$", 3)
    if algorithm != "pbkdf2_sha256":
        return False
    derived = hashlib.pbkdf2_hmac("sha256", plaintext.encode(), salt.encode(), int(iterations))
    return base64.b64encode(derived).decode() == digest


def password_changed(env_a: dict, env_b: dict) -> bool:
    for env in (env_a, env_b):
        hash_value, _ = password_rows(env)[0]
        if check_password_hash(STRONG_PASSWORD, hash_value):
            return True
    return False


def password_matches(env_a: dict, env_b: dict, plaintext: str) -> bool:
    ok = True
    for env in (env_a, env_b):
        hash_value, autoset = password_rows(env)[0]
        if not check_password_hash(plaintext, hash_value):
            print(f"FAIL: password does not verify on {env['DATABASE_URL']}")
            ok = False
        if autoset != "False":
            print(f"FAIL: is_password_autoset is {autoset}")
            ok = False
    return ok


if __name__ == "__main__":
    raise SystemExit(main())
