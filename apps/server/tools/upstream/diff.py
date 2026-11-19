#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Diff the pinned upstream commit against a later ref, classified for porting.

Usage (from anywhere):

    apps/server/tools/upstream/diff.py upstream/preview
    apps/server/tools/upstream/diff.py 18bdefae3379c6b9eea084045c2d134310ab19fb --stdout

The report is Markdown. It classifies every changed file into the buckets a
port needs (routes, serializers, models, migrations, tasks/schedules,
settings/env, middleware/auth, e-mail templates, frontends, deployment, other)
and, when the backend changed, regenerates the route inventory and introspects
the task registry of *the target's* oracle and diffs them against this
repository's `ROUTES.md` (through `routes.json`) and `TASKS.md`.

The upstream side is never checked out: both commits are exported from a bare
mirror under `~/.cache/plane-rs/git/plane.git`, and the target gets its own
oracle at `~/.cache/plane-rs/oracle/<target>/` with a virtualenv built from
that commit's `uv.lock` (see `oracle.sh`). `UPSTREAM.toml` is not edited: move
the pin with `UPGRADING.md` once the changes are ported and verified.

By default the report is written below `apps/server/target/upstream-diff/`
(gitignored). `--stdout` prints it instead; nothing generated is committed.

`--check-verbatim` is the drift guard CI runs: every path in `[verbatim]` must
be byte-identical to the pin, and every `[modified]` file must still exist on
both sides. It compares committed trees (`pin..HEAD`), not the working tree.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
from collections import OrderedDict
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve()
SERVER_DIR = HERE.parents[2]  # apps/server
REPO_ROOT = HERE.parents[4]
sys.path.insert(0, str(HERE.parent))

import oracle  # noqa: E402

ORACLE_SH = HERE.parent / "oracle.sh"
ROUTES_JSON = SERVER_DIR / "routes.json"
TASKS_MD = SERVER_DIR / "TASKS.md"
DEFAULT_REPORT_DIR = SERVER_DIR / "target" / "upstream-diff"

# Order matters: the first match wins. Backend rules apply to paths inside the
# pinned `apps/api`; frontend and deployment rules to the repository.
BACKEND_RULES: list[tuple[str, tuple[str, ...]]] = [
    ("backend tests", ("plane/tests/",)),
    ("migrations", ("migrations/", "migrations_lockfile.txt")),
    ("tasks/schedules", ("plane/bgtasks/", "plane/celery.py", "/tasks.py")),
    (
        "routes",
        ("/urls.py", "/urls/", "urls/", "/views/", "plane/urls.py"),
    ),
    ("serializers", ("serializer", "/serializers/")),
    ("models", ("plane/db/models/", "/models/", "mixins.py")),
    (
        "settings/env",
        ("plane/settings/", "/settings.py", ".env.example", "plane/conf/"),
    ),
    (
        "middleware/auth",
        (
            "/middleware",
            "plane/authentication/",
            "plane/authentication.py",
            "permissions",
            "rate_limit",
        ),
    ),
    ("e-mail templates", ("templates/", "plane/utils/email.py", "/emails/")),
]

FRONTEND_PREFIXES = (
    "apps/web/",
    "apps/space/",
    "apps/admin/",
    "apps/live/",
    "packages/",
)

DEPLOYMENT_PREFIXES = (
    "deployments/",
    "charts/",
    "apps/proxy/",
    ".github/",
    "docker-compose",
    "Dockerfile",
    "setup.sh",
    "Makefile",
    "helm",
)


def run(
    command: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        command,
        cwd=str(cwd) if cwd else None,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    if check and result.returncode != 0:
        raise SystemExit(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"{result.stdout}{result.stderr}"
        )
    return result


def git_dir() -> Path:
    """The bare mirror that holds both commits (shared with oracle.sh)."""
    path = oracle.cache_root() / "git" / "plane.git"
    if not path.is_dir():
        path.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "init", "--bare", "-q", str(path)])
    return path


def in_mirror(commit: str) -> bool:
    return (
        run(
            ["git", "-C", str(git_dir()), "cat-file", "-e", f"{commit}^{{commit}}"],
            check=False,
        ).returncode
        == 0
    )


def wait_for_commit(commit: str) -> None:
    """Put `commit` in the bare mirror, fetching from the upstream URL if needed."""
    if in_mirror(commit):
        return
    repository = oracle.upstream_repository()
    print(f"fetching {commit} from {repository}", file=sys.stderr)
    run(
        [
            "git",
            "-C",
            str(git_dir()),
            "fetch",
            "--no-tags",
            "--depth=1",
            repository,
            commit,
        ]
    )


def resolve(ref: str) -> str:
    """A commit sha for a local ref, a remote ref or a full sha."""
    if re.fullmatch(r"[0-9a-f]{40}", ref):
        return ref
    local = run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "--verify", f"{ref}^{{commit}}"],
        check=False,
    )
    if local.returncode == 0:
        return local.stdout.strip()
    repository = oracle.upstream_repository()
    for candidate in (ref, f"refs/heads/{ref}", f"refs/tags/{ref}"):
        remote = run(["git", "ls-remote", repository, candidate], check=False)
        if remote.returncode == 0 and remote.stdout.strip():
            return remote.stdout.split()[0]
    raise SystemExit(f"cannot resolve {ref!r} locally or on {repository}")


def target_env(commit: str) -> dict[str, str]:
    """The environment that points the oracle helpers at `commit`."""
    env = os.environ.copy()
    env["PLANE_RS_UPSTREAM_COMMIT"] = commit
    env["PLANE_RS_ORACLE_DIR"] = str(oracle.cache_root() / "oracle" / commit)
    return env


def fetch_target_oracle(commit: str) -> None:
    print(f"preparing the oracle for {commit[:12]}", file=sys.stderr)
    run([str(ORACLE_SH), "fetch"], env=target_env(commit))


def target_api_dir(commit: str) -> Path:
    """`apps/api` of `commit`'s oracle, without changing this process's env."""
    return Path(target_env(commit)["PLANE_RS_ORACLE_DIR"]) / "apps" / "api"


def target_python(commit: str) -> Path:
    return target_api_dir(commit) / ".venv" / "bin" / "python"


def changed_files(pin: str, target: str) -> list[dict[str, str]]:
    result = run(
        [
            "git",
            "-C",
            str(git_dir()),
            "diff",
            "--name-status",
            "--find-renames",
            "-z",
            pin,
            target,
        ]
    )
    fields = result.stdout.split("\0")
    entries: list[dict[str, str]] = []
    index = 0
    while index < len(fields) and fields[index]:
        status = fields[index]
        index += 1
        if status.startswith(("R", "C")):
            old, new = fields[index], fields[index + 1]
            index += 2
            entries.append({"status": status, "path": new, "old": old})
        else:
            entries.append({"status": status, "path": fields[index]})
            index += 1
    return entries


def classify(path: str) -> str:
    if path.startswith("apps/api/"):
        inside = path[len("apps/api/") :]
        for category, needles in BACKEND_RULES:
            if any(needle in inside for needle in needles):
                return category
        return "other backend"
    if path.startswith(FRONTEND_PREFIXES):
        return "frontends"
    if path.startswith(DEPLOYMENT_PREFIXES):
        return "deployment"
    return "other"


# ---------------------------------------------------------------------------
# Routes
# ---------------------------------------------------------------------------


def route_map(payload: dict) -> OrderedDict[tuple[str, str], tuple]:
    """(path, method) -> (view, handler, permissions) from routes.json."""
    routes: OrderedDict[tuple[str, str], tuple] = OrderedDict()
    for route in payload.get("routes", []):
        permissions = tuple(route.get("permission_classes") or ())
        method_permissions = route.get("method_permissions") or {}
        for method, handler in sorted((route.get("methods") or {}).items()):
            # An explicit per-method list (even empty) wins over the class list.
            effective = (
                method_permissions[method]
                if method in method_permissions
                else permissions
            )
            routes[(route["path"], method.upper())] = (
                route.get("view"),
                handler,
                tuple(effective),
            )
    return routes


def regenerate_routes(commit: str) -> dict:
    """Run the route inventory generator against the target oracle."""
    env = target_env(commit)
    env.update(
        {
            "DJANGO_SETTINGS_MODULE": "plane.settings.production",
            "SECRET_KEY": "upstream-diff",
            "DATABASE_URL": "postgresql://x:x@127.0.0.1:1/x",
            "REDIS_URL": "redis://127.0.0.1:1/0",
            "ENABLE_READ_REPLICA": "0",
            "ENABLE_DRF_SPECTACULAR": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    interpreter = target_python(commit)
    with tempfile.TemporaryDirectory(prefix="plane-rs-routes-") as tmp:
        run(
            [
                str(interpreter),
                str(SERVER_DIR / "tools" / "inventory" / "routes.py"),
                "--out-dir",
                tmp,
                "--commit",
                commit,
            ],
            cwd=SERVER_DIR,
            env=env,
        )
        return json.loads((Path(tmp) / "routes.json").read_text(encoding="utf-8"))


def compare_routes(commit: str) -> dict:
    baseline = route_map(json.loads(ROUTES_JSON.read_text(encoding="utf-8")))
    target = route_map(regenerate_routes(commit))
    added = [(path, method, target[(path, method)]) for path, method in sorted(set(target) - set(baseline))]
    removed = [
        (path, method, baseline[(path, method)]) for path, method in sorted(set(baseline) - set(target))
    ]
    changed = []
    for key in sorted(set(baseline) & set(target)):
        before, after = baseline[key], target[key]
        if before != after:
            changed.append((key, before, after))
    return {
        "baseline": len(baseline),
        "target": len(target),
        "added": added,
        "removed": removed,
        "changed": changed,
    }


# ---------------------------------------------------------------------------
# Tasks and schedules
# ---------------------------------------------------------------------------

TASK_INTROSPECTION = r"""
import json, os, sys
import django

django.setup()
from plane.celery import app

app.autodiscover_tasks()
app.loader.import_default_modules()

api = os.getcwd()
tasks = []
for name in sorted(app.tasks):
    task = app.tasks[name]
    if name.startswith("celery.") or name.startswith("django_celery"):
        continue
    code = getattr(task.run, "__code__", None)
    source = None
    if code is not None:
        source = os.path.relpath(code.co_filename, api).replace(os.sep, "/")
    short = name
    for prefix in ("plane.bgtasks.", "plane."):
        if short.startswith(prefix):
            short = short[len(prefix):]
            break
    tasks.append({"name": short, "full": name, "source": source})

schedule = []
for key, entry in sorted((app.conf.beat_schedule or {}).items()):
    schedule.append(
        {
            "name": key,
            "task": entry.get("task"),
            "schedule": str(entry.get("schedule")),
            "options": entry.get("options") or {},
        }
    )
print(json.dumps({"tasks": tasks, "schedule": schedule}))
"""


def introspect_tasks(commit: str) -> dict:
    env = target_env(commit)
    env.update(
        {
            "DJANGO_SETTINGS_MODULE": "plane.settings.production",
            "SECRET_KEY": "upstream-diff",
            "DATABASE_URL": "postgresql://x:x@127.0.0.1:1/x",
            "REDIS_URL": "redis://127.0.0.1:1/0",
            "ENABLE_READ_REPLICA": "0",
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    result = run(
        [str(target_python(commit)), "-c", TASK_INTROSPECTION],
        cwd=target_api_dir(commit),
        env=env,
    )
    return json.loads(result.stdout.strip().splitlines()[-1])


TASKS_MD_ROW = re.compile(
    r"^\|\s*\d+\s*\|\s*`([A-Za-z0-9_.]+)`\s*\|\s*`(plane/[^`:]+)"
)


def tasks_md() -> dict[str, str]:
    """The task table in TASKS.md: short task name -> source file."""
    found: dict[str, str] = {}
    for line in TASKS_MD.read_text(encoding="utf-8").splitlines():
        match = TASKS_MD_ROW.match(line)
        if match:
            found[match.group(1)] = match.group(2)
    return found


def compare_tasks(pin: str, commit: str) -> dict:
    documented = tasks_md()
    baseline = introspect_tasks(pin)
    target = introspect_tasks(commit)
    base_tasks = {task["name"]: task for task in baseline["tasks"]}
    target_tasks = {task["name"]: task for task in target["tasks"]}
    added = sorted(set(target_tasks) - set(base_tasks))
    removed = sorted(set(base_tasks) - set(target_tasks))
    changed = []
    for name in sorted(set(base_tasks) & set(target_tasks)):
        before, after = base_tasks[name], target_tasks[name]
        if before["source"] != after["source"]:
            changed.append((name, before["source"], after["source"]))
    undocumented = sorted(set(target_tasks) - set(documented))
    missing = sorted(set(documented) - set(target_tasks))

    schedule_added = [
        entry
        for entry in target["schedule"]
        if entry not in baseline["schedule"]
    ]
    schedule_removed = [
        entry
        for entry in baseline["schedule"]
        if entry not in target["schedule"]
    ]
    return {
        "baseline": len(base_tasks),
        "target": len(target_tasks),
        "added": added,
        "removed": removed,
        "changed": changed,
        "documented": len(documented),
        "undocumented": undocumented,
        "missing_from_target": missing,
        "schedule_added": schedule_added,
        "schedule_removed": schedule_removed,
        "schedule_target": len(target["schedule"]),
    }


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------


def category_counts(entries: list[dict[str, str]]) -> OrderedDict[str, int]:
    counts: dict[str, int] = {}
    for entry in entries:
        counts[classify(entry["path"])] = counts.get(classify(entry["path"]), 0) + 1
    order = [
        "routes",
        "serializers",
        "models",
        "migrations",
        "tasks/schedules",
        "settings/env",
        "middleware/auth",
        "e-mail templates",
        "backend tests",
        "other backend",
        "frontends",
        "deployment",
        "other",
    ]
    return OrderedDict((name, counts[name]) for name in order if name in counts)


def cell(value: object) -> str:
    return str(value if value is not None else "").replace("|", "\\|")


def permission_name(permission: object) -> str:
    if isinstance(permission, str):
        return permission.rsplit(".", 1)[-1]
    return json.dumps(permission, sort_keys=True)


def route_value(value: tuple) -> str:
    """A route's view, handler and permission classes as one table cell."""
    view, handler, permissions = value
    names = ", ".join(permission_name(permission) for permission in permissions)
    return f"`{cell(view)}` `{cell(handler)}`<br>{cell(names) or '—'}"


def render(
    target_ref: str,
    pin: str,
    target: str,
    entries: list[dict[str, str]],
    routes: dict | None,
    tasks: dict | None,
) -> str:
    lines: list[str] = []
    lines.append(f"# Upstream diff: `{pin[:12]}` → `{target_ref}`")
    lines.append("")
    lines.append(
        f"- Pinned: `{pin}` ({oracle.upstream()['upstream']['describe']}, "
        f"{oracle.upstream()['upstream']['date']})"
    )
    lines.append(f"- Target: `{target_ref}` = `{target}`")
    lines.append(
        f"- Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')} "
        "by `apps/server/tools/upstream/diff.py`"
    )
    lines.append(f"- Changed files: **{len(entries)}**")
    lines.append("")

    modified = fork_modified()
    if modified:
        changed = {entry["path"] for entry in entries}
        lines.append("## Fork-modified upstream files")
        lines.append("")
        lines.append(
            "These files are **not** verbatim: three-way merge the target's change "
            "into this fork's version (base `pinned`, ours `HEAD`, theirs `target`) "
            "and keep the documented local change. Never copy the target file over it."
        )
        lines.append("")
        lines.append("| Path | Why this fork edits it | Changed in target? |")
        lines.append("| --- | --- | --- |")
        for path, why in modified.items():
            lines.append(
                f"| `{cell(path)}` | {cell(why)} | {'**yes**' if path in changed else 'no'} |"
            )
        lines.append("")

    lines.append("## Files by category")
    lines.append("")
    lines.append("| Category | Files |")
    lines.append("| --- | ---: |")
    for name, count in category_counts(entries).items():
        lines.append(f"| {name} | {count} |")
    lines.append("")

    backend = [entry for entry in entries if entry["path"].startswith("apps/api/")]
    if routes is not None:
        lines.append("## Routes (regenerated from the target oracle)")
        lines.append("")
        lines.append(
            f"Baseline: {routes['baseline']} path×method pairs in `ROUTES.md`'s "
            f"inventory; target: {routes['target']}."
        )
        lines.append("")
        if not (routes["added"] or routes["removed"] or routes["changed"]):
            lines.append("No route added, removed or changed.")
        else:
            if routes["added"]:
                lines.append("### Added")
                lines.append("")
                lines.append("| Path | Method | View |")
                lines.append("| --- | --- | --- |")
                for path, method, value in routes["added"]:
                    lines.append(f"| `{cell(path)}` | {method} | `{cell(value[0])}` |")
                lines.append("")
            if routes["removed"]:
                lines.append("### Removed")
                lines.append("")
                lines.append("| Path | Method | View |")
                lines.append("| --- | --- | --- |")
                for path, method, value in routes["removed"]:
                    lines.append(f"| `{cell(path)}` | {method} | `{cell(value[0])}` |")
                lines.append("")
            if routes["changed"]:
                lines.append("### Changed (view, handler or permissions)")
                lines.append("")
                lines.append("| Path | Method | Before | After |")
                lines.append("| --- | --- | --- | --- |")
                for (path, method), before, after in routes["changed"]:
                    lines.append(
                        f"| `{cell(path)}` | {method} | {route_value(before)} "
                        f"| {route_value(after)} |"
                    )
                lines.append("")

    if tasks is not None:
        lines.append("## Tasks and schedules (introspected at both commits)")
        lines.append("")
        lines.append(
            f"Registered tasks: {tasks['baseline']} pinned → {tasks['target']} target. "
            f"Beat entries: {tasks['schedule_target']} at the target."
        )
        lines.append("")
        if tasks["added"]:
            lines.append("### New tasks")
            lines.append("")
            for name in tasks["added"]:
                lines.append(f"- `{name}`")
            lines.append("")
        if tasks["removed"]:
            lines.append("### Removed tasks")
            lines.append("")
            for name in tasks["removed"]:
                lines.append(f"- `{name}`")
            lines.append("")
        if tasks["changed"]:
            lines.append("### Moved tasks (source file changed)")
            lines.append("")
            lines.append("| Task | Before | After |")
            lines.append("| --- | --- | --- |")
            for name, before, after in tasks["changed"]:
                lines.append(f"| `{name}` | `{cell(before)}` | `{cell(after)}` |")
            lines.append("")
        if tasks["schedule_added"] or tasks["schedule_removed"]:
            lines.append("### Beat schedule changes")
            lines.append("")
            for entry in tasks["schedule_added"]:
                lines.append(
                    f"- added `{entry['name']}` → `{entry['task']}` "
                    f"(`{entry['schedule']}`)"
                )
            for entry in tasks["schedule_removed"]:
                lines.append(
                    f"- removed `{entry['name']}` → `{entry['task']}` "
                    f"(`{entry['schedule']}`)"
                )
            lines.append("")
        if tasks["undocumented"] or tasks["missing_from_target"]:
            lines.append("### TASKS.md drift")
            lines.append("")
            if tasks["undocumented"]:
                lines.append(
                    "Registered at the target but absent from `TASKS.md`: "
                    + ", ".join(f"`{name}`" for name in tasks["undocumented"])
                )
            if tasks["missing_from_target"]:
                lines.append(
                    "Documented in `TASKS.md` but not registered at the target: "
                    + ", ".join(f"`{name}`" for name in tasks["missing_from_target"])
                )
            lines.append("")
        else:
            lines.append(
                f"`TASKS.md` accounts for all {tasks['target']} registered target tasks "
                f"({tasks['documented']} table rows)."
            )
            lines.append("")

    if backend:
        migrations = [
            entry
            for entry in backend
            if classify(entry["path"]) == "migrations"
            and entry["status"].startswith(("A", "M", "R"))
        ]
        lines.append("## Django migrations")
        lines.append("")
        if migrations:
            lines.append("| Status | Migration |")
            lines.append("| --- | --- |")
            for entry in migrations:
                path = entry["path"]
                if entry.get("old"):
                    path = f"{entry['old']} → {path}"
                lines.append(f"| {entry['status']} | `{cell(path)}` |")
            lines.append("")
            lines.append(
                "Port them as ordered migrator upgrade steps; see `UPGRADING.md`."
            )
        else:
            lines.append("No migration added, changed or removed.")
        lines.append("")

    lines.append("## Every changed file")
    lines.append("")
    lines.append("| Status | Category | Path |")
    lines.append("| --- | --- | --- |")
    for entry in sorted(entries, key=lambda item: (classify(item["path"]), item["path"])):
        path = entry["path"]
        if entry.get("old"):
            path = f"{entry['old']} → {path}"
        lines.append(
            f"| {entry['status']} | {classify(entry['path'])} | `{cell(path)}` |"
        )
    lines.append("")
    return "\n".join(lines)


def fork_modified() -> dict[str, str]:
    """The `[modified]` table: upstream path -> why this fork edits it."""
    return dict(oracle.upstream().get("modified", {}))


def check_verbatim() -> int:
    """Fail when a `[verbatim]` path differs from the pin or a modified file vanished."""
    pin = oracle.pinned_commit()
    # Compare in the real checkout: the pin is in this repository's history (or
    # is fetched into it), while the bare mirror has no branch HEAD for a fork
    # commit. Fetching the pin by SHA is the one allowed network operation.
    if (
        run(
            ["git", "-C", str(REPO_ROOT), "cat-file", "-e", f"{pin}^{{commit}}"],
            check=False,
        ).returncode
        != 0
    ):
        wait_for_commit(pin)
        run(
            [
                "git",
                "-C",
                str(REPO_ROOT),
                "fetch",
                "--no-tags",
                "--depth=1",
                oracle.upstream_repository(),
                pin,
            ]
        )
    data = oracle.upstream()
    paths = list(data.get("verbatim", {}).get("paths", []))
    modified = fork_modified()
    if not paths:
        print("FAIL: [verbatim] lists no paths", file=sys.stderr)
        return 1

    overlap = sorted(set(paths) & set(modified))
    if overlap:
        print(
            "FAIL: paths are both verbatim and modified: " + ", ".join(overlap),
            file=sys.stderr,
        )
        return 1

    # The bare mirror has no branch HEAD; compare against this checkout's
    # committed HEAD explicitly.
    head = run(
        ["git", "-C", str(REPO_ROOT), "rev-parse", "--verify", "HEAD^{commit}"]
    ).stdout.strip()
    result = run(
        ["git", "-C", str(REPO_ROOT), "diff", "--name-status", pin, head, "--", *paths],
        check=False,
    )
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr)
        return 1
    if result.stdout.strip():
        print(
            "FAIL: these [verbatim] paths differ from the pin "
            f"{pin[:12]}; either bring them back or list the file under [modified] "
            "with its reason:",
            file=sys.stderr,
        )
        print(result.stdout.rstrip(), file=sys.stderr)
        return 1

    failures = 0
    for path in modified:
        for ref in (pin, head):
            present = (
                run(
                    ["git", "-C", str(REPO_ROOT), "cat-file", "-e", f"{ref}:{path}"],
                    check=False,
                ).returncode
                == 0
            )
            if not present:
                print(f"FAIL: [modified] {path} is missing at {ref}", file=sys.stderr)
                failures += 1
    if failures:
        return 1

    print(
        f"ok: [verbatim] {len(paths)} path(s) match {pin[:12]}; "
        f"[modified] {len(modified)} file(s) exist on both sides"
    )
    return 0


def safe_name(ref: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", ref).strip("_") or "target"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "target_ref",
        nargs="?",
        help="a sha, a local ref or an upstream branch (not needed with --check-verbatim)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="report path (default: apps/server/target/upstream-diff/<ref>.md)",
    )
    parser.add_argument("--stdout", action="store_true", help="print instead of writing")
    parser.add_argument(
        "--no-inventories",
        action="store_true",
        help="only classify files; skip the oracle, route and task regeneration",
    )
    parser.add_argument(
        "--check-verbatim",
        action="store_true",
        help="fail when a [verbatim] path differs from the pin",
    )
    args = parser.parse_args(argv)

    if args.check_verbatim:
        return check_verbatim()

    if not args.target_ref:
        parser.error("target_ref is required unless --check-verbatim is used")

    pin = oracle.pinned_commit()
    target = resolve(args.target_ref)
    if target == pin:
        raise SystemExit(f"{args.target_ref} is the pinned commit")
    wait_for_commit(pin)
    wait_for_commit(target)

    entries = changed_files(pin, target)
    routes = tasks = None
    if not args.no_inventories and any(
        entry["path"].startswith("apps/api/") for entry in entries
    ):
        # The pin's oracle is needed for the task/schedule baseline.
        run([str(ORACLE_SH), "fetch"])
        fetch_target_oracle(target)
        routes = compare_routes(target)
        tasks = compare_tasks(pin, target)

    report = render(args.target_ref, pin, target, entries, routes, tasks)
    if args.stdout:
        print(report)
        return 0
    out = args.out or (DEFAULT_REPORT_DIR / f"{safe_name(args.target_ref)}.md")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(report, encoding="utf-8")
    print(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
