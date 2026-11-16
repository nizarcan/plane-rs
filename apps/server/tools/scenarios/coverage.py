#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run the generated read scenarios against Django and write COVERAGE.md.

For every `scenarios/read/<group>.yaml` this runs

    difftest run --scenario <file> --a django --b django --report <json>

twice (so that a difference caused by a flaky read is visible), collects the A
side's status and body per step, and writes `scenarios/read/COVERAGE.md`:

* per group: steps, pass/fail, wall-clock runtime;
* per route: number of requests, status distribution, how many 200s carried a
  non-empty payload;
* the routes for which no 200 with data was produced, with the reason.

`scenarios/read/route-index.json` (written by `gen_read.py`) maps each step name
to its route template.

Usage (from `apps/server`; the stack and `plane_fix_tpl` must exist):

    apps/api/.venv/bin/python tools/scenarios/coverage.py
    apps/api/.venv/bin/python tools/scenarios/coverage.py --only issues --runs 1
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from collections import Counter, OrderedDict
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
README_DIR = SERVER_DIR / "scenarios" / "read"
INDEX = README_DIR / "route-index.json"
DIFTEST = SERVER_DIR / "target" / "debug" / "difftest"
OUT = README_DIR / "COVERAGE.md"
TIMINGS = README_DIR / "coverage-timings.json"

# Reasons a route cannot return 200 with data (route template -> reason).
NOTES = {
    "/api/workspaces/": "Django answers 400: `allow_permission` reads `kwargs['slug']` on a route without it (upstream bug).",
    "/api/workspaces/{slug}/file-assets/": "Collection route has only POST/DELETE handlers; GET is 405.",
    "/api/users/file-assets/": "Collection route has only POST; GET is 405.",
    "/api/assets/v2/workspaces/{slug}/": "Workspace asset collection has no GET handler (405).",
    "/api/assets/v2/workspaces/{slug}/{asset_id}/": "Workspace asset detail has no GET handler (405).",
    "/api/public/assets/v2/anchor/{anchor}/": "`EntityAssetEndpoint.get` requires `pk`; the URL without it raises a TypeError (500).",
    "/api/workspaces/{slug}/user-favorites/{favorite_id}/": "500: `WorkspaceFavoriteEndpoint.get()` has no `favorite_id` parameter (`plane/app/views/workspace/favorite.py:25`), so the detail route raises TypeError.",
    "/api/workspaces/{slug}/home-preferences/{key}/": "500: `WorkspaceHomePreferenceViewSet.get()` has no `key` parameter (`plane/app/views/workspace/home.py:24`), so the detail route raises TypeError.",
    "/api/v1/workspaces/{slug}/invitations.{format}": "500: `WorkspaceInvitationsViewset.list()` has no `format` parameter, so the DRF format-suffix route raises TypeError (`plane/api/views/invite.py`).",
    "/api/v1/workspaces/{slug}/invitations/{pk}.{format}": "500: same missing `format` parameter on the format-suffix route.",
    "/api/v1/workspaces/{slug}/stickies.{format}": "500: `StickyViewSet.list()` has no `format` parameter (`plane/api/views/sticky.py`).",
    "/api/v1/workspaces/{slug}/stickies/{pk}.{format}": "500: same missing `format` parameter on the format-suffix route.",
    "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/": "200 with [] for every request: `IssueReactionPublicViewSet.get_queryset` looks the board up by `workspace__slug`/`project_id` kwargs that this route does not have (`plane/space/views/issue.py:401-404`), so it always returns an empty queryset.",
    "/api/public/anchor/{anchor}/issues/{issue_id}/votes/": "200 with [] for every request: `IssueVotePublicViewSet.get_queryset` filters `workspace__slug=<anchor>` instead of the board's workspace (`plane/space/views/issue.py:562`), so it always returns an empty queryset.",
    "/api/users/last-visited-workspace/": "500: the view reads `user.last_workspace_id`, but `last_workspace_id` is a `Profile` field, not a `User` field (`plane/app/views/workspace/user.py:89`; model at `plane/db/models/user.py:236`).",
    "/api/public/workspaces/{slug}/project-boards/": "500: `DeployBoard.objects.filter(...).values_list` is not called and `.workspace` is read off the QuerySet (`plane/space/views/project.py:32-34`).",
    "/api/instances/admins/{pk}/": "500: `InstanceAdminEndpoint.get()` has no `pk` parameter, so the detail route raises TypeError (`plane/license/api/views/admin.py:73`).",
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": "500: `IssueAttachmentEndpoint.get()` has no `pk` parameter, so the detail route raises TypeError (`plane/app/views/issue/attachment.py:100`).",
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/": "No GET handler for the archive action; the route is registered for GET (405/500).",
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/": "No GET handler for the archive action; the route is registered for GET (405/500).",
    "/api/workspaces/{slug}/export-issues/": "`ExportIssuesEndpoint` only implements POST (`plane/app/views/exporter/base.py:24`); GET is 400 through the view's error handler.",
    "/api/workspaces/{slug}/users/notifications/{pk}/": "405: `retrieve` is served by DRF's mixin and the `BaseViewSet` guard refuses a mixin action with only `IsAuthenticated` (ROUTES.md lists these 405s).",
    "/api/workspaces/{slug}/stickies/{pk}/": "405: `retrieve` is a DRF mixin action with no authorization; refused by the `BaseViewSet` guard.",
    "/api/workspaces/{slug}/user-favorite-projects/": "Only POST/DELETE (the favourite list is `/user-favorites/`); GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/": "Only POST/DELETE; GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/": "Only POST/DELETE; GET is 405/500.",
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/": "Only POST/DELETE; GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/": "Only POST/DELETE (add/remove an issue); GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/": "Only POST/DELETE (add/remove an issue); GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/": "Only POST/PATCH/DELETE on an intake; GET is 405.",
    "/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/": "Only POST/PATCH/DELETE on an inbox; GET is 405.",
    "/api/unsplash/": "200 with []: the view proxies Unsplash and the dev stack has no Unsplash credentials/network access, so the upstream result is empty.",
    "/api/assets/v2/static/{asset_id}/": "302 redirect to a presigned object URL (data-bearing; the body is empty by design).",
    "/api/assets/v2/workspaces/{slug}/download/{asset_id}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/public/assets/v2/anchor/{anchor}/{pk}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/": "`ProjectAssetEndpoint` implements POST/PATCH/DELETE only; GET raises through the view's error handler (500).",
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/": "The v2 attachment endpoint implements POST/PATCH/DELETE only; the 200s are the list route's empty result for issues without attachments.",
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/": "302 redirect to a presigned object URL (data-bearing).",
    "/api/v1/workspaces/{slug}/stickies/{pk}.{format}": "500: `StickyViewSet.retrieve()` has no `format` parameter on the format-suffix route (`plane/api/views/sticky.py`).",
}

# OAuth initiators and callbacks are 302 by design: with no provider configured
# in the fixture instance they redirect back to the web app with an error code.
for _path in (
    "/auth/google/",
    "/auth/google/callback/",
    "/auth/github/",
    "/auth/github/callback/",
    "/auth/gitlab/",
    "/auth/gitlab/callback/",
    "/auth/gitea/",
    "/auth/gitea/callback/",
):
    NOTES.setdefault(_path, "302 redirect: the provider is not configured in the fixture instance, so the view redirects back to the web app instead of to the provider.")
    NOTES.setdefault(_path.replace("/auth/", "/auth/spaces/"), "302 redirect: the provider is not configured in the fixture instance (spaces variant).")


def body_is_non_empty(body: str | None) -> bool:
    if body is None:
        return False
    text = body.strip()
    if text in {"", "[]", "{}", "null", '""', "None"}:
        return False
    if text.startswith("["):
        return text != "[]"
    if text.startswith("{"):
        return text != "{}"
    return True


def load_timings() -> dict:
    if not TIMINGS.is_file():
        return {}
    try:
        return json.loads(TIMINGS.read_text())
    except json.JSONDecodeError:
        return {}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", default=None, help="limit to one group")
    parser.add_argument("--runs", type=int, default=2, help="how many Django-vs-Django runs per group (default 2)")
    parser.add_argument("--report-dir", type=Path, default=Path("/tmp/read-coverage"), help="where to keep reports")
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="rebuild COVERAGE.md from the reports already in --report-dir instead of running the suite",
    )
    return parser.parse_args()


def run_group(group: str, runs: int, report_dir: Path) -> dict:
    """Run one scenario `runs` times, retrying harness-level failures.

    A retry only happens when the harness wrote **no report**: that is a
    startup/infrastructure failure (a port still held by the previous target, a
    database clone that lost a race), not a difference. A run that produced a
    report is never retried, so a real DIFF cannot be hidden.
    """
    scenario = README_DIR / f"{group}.yaml"
    report_dir.mkdir(parents=True, exist_ok=True)
    results = []
    for run in range(1, runs + 1):
        report = report_dir / f"{group}-{run}.json"
        if report.exists():
            report.unlink()
        attempts = 0
        while True:
            attempts += 1
            started = time.monotonic()
            completed = subprocess.run(
                [
                    str(DIFTEST),
                    "run",
                    "--scenario",
                    str(scenario),
                    "--a",
                    "django",
                    "--b",
                    "django",
                    "--report",
                    str(report),
                ],
                cwd=SERVER_DIR,
                capture_output=True,
                text=True,
            )
            elapsed = time.monotonic() - started
            if report.is_file() or attempts >= 3:
                break
            print(f"  {group} run {run}: harness failed without a report, retrying ({attempts}/3)")
            time.sleep(2)
        results.append(
            {
                "run": run,
                "exit": completed.returncode,
                "seconds": elapsed,
                "attempts": attempts,
                "report": report if report.is_file() else None,
                "stdout": completed.stdout,
                "stderr": completed.stderr,
            }
        )
    return {"group": group, "runs": results}


def load_report(path: Path | None) -> dict | None:
    if path is None:
        return None
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def main() -> int:
    args = parse_args()
    if not INDEX.is_file():
        raise SystemExit(f"{INDEX} is missing; run tools/scenarios/gen_read.py first")
    if not DIFTEST.is_file():
        raise SystemExit(f"{DIFTEST} is missing; cargo build -p plane-testkit first")
    index = json.loads(INDEX.read_text())
    groups = sorted(index["routes"])
    if args.only:
        groups = [group for group in groups if group == args.only]
        if not groups:
            raise SystemExit(f"unknown group {args.only!r}")

    all_results = []
    total_seconds = 0.0
    timings: dict = {}
    for group in groups:
        if args.reuse:
            saved = load_timings()
            runs = []
            for run_number in range(1, args.runs + 1):
                report = args.report_dir / f"{group}-{run_number}.json"
                runs.append(
                    {
                        "run": run_number,
                        "exit": 0 if report.is_file() else 1,
                        "seconds": saved.get(group, {}).get(f"run{run_number}", 0.0),
                        "attempts": 1,
                        "report": report if report.is_file() else None,
                        "stdout": "",
                        "stderr": "",
                    }
                )
            result = {"group": group, "runs": runs}
        else:
            result = run_group(group, args.runs, args.report_dir)
            timings[group] = {f"run{run['run']}": round(run['seconds'], 1) for run in result["runs"]}
        all_results.append(result)
        total_seconds += sum(run["seconds"] for run in result["runs"])
        print(f"{group:16s} " + " ".join(
            f"run{run['run']}:{'ok' if run['exit'] == 0 else 'FAIL'}({run['seconds']:.1f}s)"
            for run in result["runs"]
        ))

    # Per-route coverage is taken from the first run of each group (the second
    # run exists to prove the reads are deterministic).
    routes: OrderedDict[str, dict] = OrderedDict()
    for result in all_results:
        group = result["group"]
        report = load_report(result["runs"][0]["report"])
        if report is None:
            continue
        step_reports = {step["name"]: step for step in report["steps"]}
        for entry in index["routes"][group]:
            key = entry.get("route", entry["path"])
            route = routes.setdefault(
                key,
                {
                    "group": group,
                    "path": key,
                    "statuses": Counter(),
                    "requests": 0,
                    "with_data": 0,
                    "identities": Counter(),
                },
            )
            route["requests"] += 1
            route["identities"][entry["identity"]] += 1
            step = step_reports.get(entry["step"])
            if step is None:
                continue
            status = step["a"]["status"]
            route["statuses"][status] += 1
            if status == 200 and body_is_non_empty(step["a"].get("body")):
                route["with_data"] += 1

    failures = [result for result in all_results if any(run["exit"] != 0 for run in result["runs"])]
    lines: list[str] = []
    lines.append("# Read (GET) scenario coverage")
    lines.append("")
    lines.append("Generated by `apps/server/tools/scenarios/coverage.py`; the scenarios")
    lines.append("themselves are generated by `apps/server/tools/scenarios/gen_read.py`.")
    lines.append("")
    lines.append(f"- Fixture template: `{index['template_db']}` (base time `{index['base_time']}`)")
    lines.append(f"- Groups: {len(all_results)}; steps per run: {sum(len(index['routes'][g]) for g in groups)}")
    lines.append(f"- Runs per group against Django: {args.runs}")
    lines.append(f"- Total wall clock for all runs: {total_seconds:.1f} s")
    lines.append("")
    total_statuses: Counter = Counter()
    total_requests = 0
    for route in routes.values():
        total_statuses.update(route["statuses"])
        total_requests += route["requests"]
    with_data = sum(1 for route in routes.values() if route["with_data"])
    lines.append("## Summary")
    lines.append("")
    lines.append(f"- Requests per pass: {total_requests}; routes: {len(routes)}")
    lines.append(
        f"- Routes with at least one 200 carrying data: {with_data} / {len(routes)} "
        f"({100 * with_data // max(len(routes), 1)} %)"
    )
    lines.append(
        "- Status distribution on Django (first pass): "
        + ", ".join(f"{status}×{count}" for status, count in sorted(total_statuses.items()))
    )
    lines.append("")
    lines.append("## Groups")
    lines.append("")
    lines.append("| group | routes | steps | run 1 | run 2 | seconds |")
    lines.append("| --- | ---: | ---: | --- | --- | ---: |")
    for result in all_results:
        group = result["group"]
        marks = []
        for run in result["runs"]:
            marks.append("ok" if run["exit"] == 0 else "**DIFF**")
        while len(marks) < 2:
            marks.append("-")
        distinct = len({entry["path"] for entry in index["routes"][group]})
        seconds = sum(run["seconds"] for run in result["runs"])
        lines.append(
            f"| {group} | {distinct} | {len(index['routes'][group])} | {marks[0]} | {marks[1]} | {seconds:.1f} |"
        )
    lines.append("")
    lines.append("## Per-route")
    lines.append("")
    lines.append("`requests` counts every generated step for the route; `statuses` is the")
    lines.append("distribution of Django's HTTP status on the first run; `200 with data`")
    lines.append("counts 200s whose body is not empty (`[]`/`{}`/`null`).")
    lines.append("")
    lines.append("| group | route | requests | statuses | 200 with data |")
    lines.append("| --- | --- | ---: | --- | ---: |")
    missing: list[dict] = []
    for route in routes.values():
        statuses = ", ".join(f"{status}×{count}" for status, count in sorted(route["statuses"].items()))
        lines.append(
            f"| {route['group']} | `{route['path']}` | {route['requests']} | {statuses} | {route['with_data']} |"
        )
        if route["with_data"] == 0:
            missing.append(route)
    lines.append("")
    lines.append("## Routes without a 200 carrying data")
    lines.append("")
    if not missing:
        lines.append("None.")
    else:
        lines.append("| group | route | statuses | reason |")
        lines.append("| --- | --- | --- | --- |")
        for route in missing:
            reason = NOTES.get(route["path"], _infer_reason(route))
            statuses = ", ".join(f"{status}×{count}" for status, count in sorted(route["statuses"].items()))
            lines.append(f"| {route['group']} | `{route['path']}` | {statuses} | {reason} |")
    lines.append("")
    lines.append("## Notes")
    lines.append("")
    lines.append("- Sessions come from the fixture as an explicit `Cookie` header, so no step writes.")
    lines.append("- Every request carries `Host: plane-fixture.test`: Django's `cache_page` key")
    lines.append("  hashes `request.build_absolute_uri()`, the S3 presigner signs")
    lines.append("  `request.get_host()` (`plane/settings/storage.py:54`), and the `/api/v1/` log")
    lines.append("  message records the Host header. Without a fixed Host those values differ")
    lines.append("  between two targets purely because their ports differ.")
    lines.append("- The scenarios use Redis DBs 6 and 7. The harness default `[1, 2]` is shared")
    lines.append("  by every agent on the dev stack, and a concurrent run that writes a")
    lines.append("  `cache_page` key between our A and B steps turns into a Redis DIFF that")
    lines.append("  exists only because two runs share an index.")
    lines.append("- `PYTHONHASHSEED=0` is set for both targets: some views iterate a Python")
    lines.append("  `set` when building their response (`ProjectSummaryAPIEndpoint`), so JSON key")
    lines.append("  order otherwise follows the process's hash seed.")
    lines.append("- Cyclic windows are relative to the fixture build time; no scenario depends")
    lines.append("  on the exact day a cycle is read.")
    lines.append("")
    if failures:
        lines.append("## Failing groups")
        lines.append("")
        for result in failures:
            group = result["group"]
            lines.append(f"### {group}")
            lines.append("")
            for run in result["runs"]:
                if run["exit"] == 0:
                    continue
                diff_lines = [line for line in run["stdout"].splitlines() if "DIFF" in line]
                lines.append(f"- run {run['run']}: {len(diff_lines)} differing step(s)")
                for line in diff_lines[:20]:
                    lines.append(f"  - `{line.strip()}`")
            lines.append("")

    if timings and not args.reuse:
        TIMINGS.write_text(json.dumps(timings, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    args.out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {args.out}")
    return 0 if not failures else 1


def _infer_reason(route: dict) -> str:
    statuses = route["statuses"]
    if not statuses:
        return "no step reached Django"
    if set(statuses) == {404}:
        return "404: the bound fixture object does not exist for this route/user (wrong parent project or owner)."
    if set(statuses) == {403}:
        return "403: the fixture user lacks the permission this route requires."
    if set(statuses) == {405}:
        return "405: the route has no GET handler for this method table."
    if 500 in statuses:
        return "500: the view raises for this request (upstream bug or a missing fixture row)."
    if 200 in statuses:
        return "200 with an empty body for every query variant generated."
    return "unexpected status; investigate."


if __name__ == "__main__":
    raise SystemExit(main())
