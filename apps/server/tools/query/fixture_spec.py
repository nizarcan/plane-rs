# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Shared constants for the `plane-query` fixture, oracle and benchmarks.

Everything the fixture creates has a deterministic id derived from a name, and
every timestamp is derived from :data:`FIXED_NOW` rather than the wall clock,
so the golden files are reproducible: build the fixture again and the oracle
emits the same bytes.

Relative date filters (`1_weeks;after;fromnow` and friends) read
``django.utils.timezone.now()``. The oracle freezes that call to
:data:`FIXED_NOW` (see ``_freeze_now`` in ``oracle.py``) and records it in the
golden metadata; the Rust side passes the same instant, so a relative filter is
tested without the test becoming time-dependent.
"""

import uuid
from datetime import datetime, timezone as dt_timezone

#: The instant every generated timestamp is measured from, and the instant the
#: oracle pins ``timezone.now()`` to while it evaluates relative-date filters.
FIXED_NOW = datetime(2026, 6, 15, 12, 0, 0, tzinfo=dt_timezone.utc)

#: Seed for every random choice in the builder.
SEED = 20261116

#: Namespace for deterministic UUIDs. Any fixed UUID works; ``uuid5`` over a
#: readable name keeps the fixture inspectable.
_NAMESPACE = uuid.UUID("6f5c2b1e-9d4a-5c3b-8a7e-0d1f2e3a4b5c")


def uid(name: str) -> uuid.UUID:
    """The deterministic UUID for ``name``."""
    return uuid.uuid5(_NAMESPACE, name)


WORKSPACE_SLUG = "query-fixture"
OTHER_WORKSPACE_SLUG = "query-other"
PROJECT_IDENTIFIER = "QFIX"
PROJECT2_IDENTIFIER = "QFIX2"

#: Issues created in the main project.
MAIN_ISSUE_COUNT = 300
#: Issues created in the second project of the same workspace.
SECOND_ISSUE_COUNT = 60

# Named ids the oracle and the Rust tests refer to.
WORKSPACE_ID = uid("workspace")
OTHER_WORKSPACE_ID = uid("workspace:other")
PROJECT_ID = uid("project")
PROJECT2_ID = uid("project:second")
ARCHIVED_PROJECT_ID = uid("project:archived")
OTHER_PROJECT_ID = uid("project:other-workspace")

ALICE = uid("user:alice")  # UTC
BOB = uid("user:bob")  # Asia/Tokyo
CAROL = uid("user:carol")  # America/Los_Angeles
DAVE = uid("user:dave")  # not a project member

USERS = {
    "alice": (ALICE, "UTC"),
    "bob": (BOB, "Asia/Tokyo"),
    "carol": (CAROL, "America/Los_Angeles"),
    "dave": (DAVE, "Europe/Berlin"),
}

#: State names per project. ``triage`` is a triage state and is invisible to
#: ``Issue.issue_objects``; ``deleted`` is soft-deleted.
PROJECT_STATES = [
    ("triage", "triage", True, False),
    ("backlog", "backlog", False, False),
    ("todo", "unstarted", False, False),
    ("in-progress", "started", False, False),
    ("done", "completed", False, False),
    ("cancelled", "cancelled", False, False),
    ("deleted", "started", False, True),
]

PROJECT2_STATES = [
    ("p2-backlog", "backlog", False, False),
    ("p2-started", "started", False, False),
    ("p2-done", "completed", False, False),
]

LABELS = [
    "alpha-label",
    "beta-label",
    "gamma-label",
    "delta-label",
    "eps-label",
    "deleted-label",
    "p2-label-one",
    "p2-label-two",
    "workspace-label",
    "other-workspace-label",
]

MODULES = ["module-alpha", "module-beta", "module-archived", "module-deleted", "p2-module"]
CYCLES = ["cycle-alpha", "cycle-beta", "cycle-deleted", "p2-cycle"]

PRIORITIES = ["urgent", "high", "medium", "low", "none"]


def state_id(project: str, name: str) -> uuid.UUID:
    return uid(f"state:{project}:{name}")


def state_ids(project: str) -> dict[str, uuid.UUID]:
    names = [row[0] for row in PROJECT_STATES] if project == "main" else [row[0] for row in PROJECT2_STATES]
    return {name: state_id(project, name) for name in names}


def label_id(name: str) -> uuid.UUID:
    return uid(f"label:{name}")


def module_id(name: str) -> uuid.UUID:
    return uid(f"module:{name}")


def cycle_id(name: str) -> uuid.UUID:
    return uid(f"cycle:{name}")


def issue_id(index: int, project: str = "main") -> uuid.UUID:
    return uid(f"issue:{project}:{index:04d}")


def estimate_point_id(key: int) -> uuid.UUID:
    return uid(f"estimate-point:{key}")
