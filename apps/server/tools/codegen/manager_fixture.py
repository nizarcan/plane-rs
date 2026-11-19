#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Build the manager-predicate fixture and its expected id sets.

`plane-db`'s manager predicates (the `objects`/`all_objects` filters and the
`Issue`/`State` custom managers) are generated from Django's own querysets.
This script proves the translation: it fills a disposable clone with one live
and one soft-deleted row for every soft-deletable model, adds the rows the
non-trivial managers need (triage states, archived/draft/triage issues,
archived projects, issues with a NULL state), and records what each Django
manager returns in `crates/plane-db/tests/golden/managers.json`.

It refuses to touch anything whose database name does not start with `t_`
(see `apps/server/AGENTS.md`, "Isolation between concurrent agents"). Run it
on a clone of the template::

    apps/server/dev/mkdb.sh t_models_1
    cd apps/api
    DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=fixture \\
    DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_models_1 \\
    .venv/bin/python ../server/tools/codegen/manager_fixture.py
    cd ../server
    PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_models_1 \\
      PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-db --test models

Rows are created with `bulk_create` so no `save()` hook runs: this fixture
tests manager filters, not write hooks (those are covered by their own tests).
Primary keys and unique text values are deterministic, so rerunning the script
on a fresh clone of the same template produces the same golden file. The
template's own rows (the bootstrapped `instances` and
`instance_configurations`) are part of the expected id sets; rebuilding
`plane_tpl` with a new instance id requires regenerating the golden.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import sys
import uuid
from decimal import Decimal
from pathlib import Path
from typing import Any

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]
# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(REPO_ROOT / "apps" / "server" / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
GOLDEN = SERVER_DIR / "crates" / "plane-db" / "tests" / "golden" / "managers.json"
MARKER_SLUG = "models-manager-fixture"
UUID_NAMESPACE = uuid.UUID("9b3a6b8e-6a4c-5d1e-8f2a-0c7d5e4b3a21")


def setup_django() -> Any:
    if str(API_DIR) not in sys.path:
        sys.path.insert(0, str(API_DIR))
    if str(SCRIPT_PATH.parent) not in sys.path:
        sys.path.insert(0, str(SCRIPT_PATH.parent))
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "fixture")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

    import django

    django.setup()
    return django


class Fixture:
    def __init__(self) -> None:
        from django.db.models import NOT_PROVIDED

        self.NOT_PROVIDED = NOT_PROVIDED
        self.cache: dict[tuple[str, str], Any] = {}
        self.next_pk = 900_000
        self.created = 0

    # -- value generation -------------------------------------------------

    @staticmethod
    def digest(key: str) -> str:
        return hashlib.sha256(key.encode("utf-8")).hexdigest()

    def text_value(self, model: Any, field: Any, variant: str, scope: str, max_length: int | None) -> str:
        key = f"{model._meta.label_lower}:{field.name}:{variant}:{scope}"
        value = "fx" + self.digest(key)
        if max_length is not None:
            return value[:max_length]
        return value

    @staticmethod
    def primary_key(model: Any, variant: str, scope: str) -> Any:
        pk = model._meta.pk
        if pk is None:
            raise SystemExit(f"{model._meta.label} has no primary key")
        name = type(pk).__name__
        key = f"{model._meta.label_lower}:{variant}:{scope}"
        if name == "UUIDField":
            return str(uuid.uuid5(UUID_NAMESPACE, key))
        if name in ("AutoField", "BigAutoField", "SmallAutoField"):
            raise AssertionError("integer primary keys are assigned by the fixture")
        if name in ("CharField", "SlugField"):
            return ("fx-pk-" + Fixture.digest(key))[: (pk.max_length or 255)]
        raise SystemExit(f"unsupported primary key {name} on {model._meta.label}")

    def scalar_for(self, model: Any, field: Any, variant: str, scope: str) -> Any:
        from django.contrib.postgres.fields import ArrayField

        name = type(field).__name__
        if isinstance(field, ArrayField):
            return []
        if name in ("CharField", "SlugField"):
            return self.text_value(model, field, variant, scope, field.max_length)
        if name == "EmailField":
            local = self.text_value(model, field, variant, scope, (field.max_length or 255) - 12)
            return f"{local}@example.com"
        if name == "URLField":
            value = "https://example.com/" + self.text_value(model, field, variant, scope, None)
            return value[: (field.max_length or 200)]
        if name in ("FileField", "ImageField"):
            value = "fixtures/" + self.text_value(model, field, variant, scope, None) + ".txt"
            return value[: (field.max_length or 100)]
        if name == "TextField":
            return "fixture"
        if name == "BinaryField":
            return b"fixture"
        if name == "BooleanField":
            return False
        if name in ("SmallIntegerField", "PositiveSmallIntegerField"):
            return 1
        if name in (
            "IntegerField",
            "PositiveIntegerField",
            "BigIntegerField",
            "PositiveBigIntegerField",
            "AutoField",
        ):
            return 1
        if name == "FloatField":
            return 1.0
        if name == "DecimalField":
            return Decimal("1.5")
        if name == "JSONField":
            return {}
        if name == "DateTimeField":
            from django.utils import timezone

            return timezone.now()
        if name == "DateField":
            from django.utils import timezone

            return timezone.localdate()
        if name == "TimeField":
            return datetime.time(0, 0)
        if name == "UUIDField":
            return self.primary_key(model, variant, scope)
        if name == "GenericIPAddressField":
            return "127.0.0.1"
        raise SystemExit(
            f"no fixture value for {model._meta.label}.{field.name} ({type(field).__name__})"
        )

    # -- creation ---------------------------------------------------------

    def dependency(self, model: Any, scope: str = "shared") -> Any:
        key = (model._meta.label_lower, scope)
        if key not in self.cache:
            self.cache[key] = self.create(model, f"dep-{len(self.cache)}", scope=scope)
        return self.cache[key]

    def create(
        self,
        model: Any,
        variant: str,
        *,
        scope: str = "",
        deleted: bool = False,
        **overrides: Any,
    ) -> Any:
        scope = scope or variant
        values: dict[str, Any] = {}
        for field in model._meta.concrete_fields:
            if field.attname in overrides:
                values[field.attname] = overrides[field.attname]
                continue
            if field.primary_key:
                if type(field).__name__ in ("AutoField", "BigAutoField", "SmallAutoField"):
                    self.next_pk += 1
                    values[field.attname] = self.next_pk
                else:
                    values[field.attname] = self.primary_key(model, variant, scope)
                continue
            if field.name == "deleted_at" and deleted:
                from django.utils import timezone

                values[field.attname] = timezone.now()
                continue
            if field.default is not self.NOT_PROVIDED and not (
                field.default is None and not field.null
            ):
                continue
            if field.null:
                continue
            if field.is_relation:
                target = field.related_model
                # A fresh target per owning row: constraints such as
                # GithubCommentSync(issue_sync, comment) have no deleted_at
                # column, so the live and soft-deleted rows must not share
                # their parents.
                dependency_scope = f"{model._meta.label_lower}:{variant}"
                values[field.attname] = self.dependency(target, dependency_scope).pk
                continue
            values[field.attname] = self.scalar_for(model, field, variant, scope)

        instance = model(**values)
        model._base_manager.bulk_create([instance])
        self.created += 1
        return instance


def build_rows(fixture: Fixture, models: list[Any]) -> None:
    from plane.db.models import Issue, Project, State, Workspace

    # A registered marker row; the Rust test only runs when it can find it.
    workspace = fixture.dependency(Workspace)
    fixture.create(
        Workspace,
        "marker",
        slug=MARKER_SLUG,
        name="Models fixture",
        owner_id=workspace.owner_id,
    )

    for model in sorted(models, key=lambda item: item._meta.label_lower):
        fixture.create(model, "live")
        if any(field.name == "deleted_at" for field in model._meta.concrete_fields):
            fixture.create(model, "deleted", deleted=True)

    # Non-trivial manager cases.
    project = fixture.dependency(Project)
    workspace_id = project.workspace_id
    triage = fixture.create(
        State,
        "triage",
        project_id=project.pk,
        workspace_id=workspace_id,
        name="Fixture triage state",
        group="triage",
        is_triage=True,
    )
    triage_deleted = fixture.create(
        State,
        "triage-deleted",
        deleted=True,
        project_id=project.pk,
        workspace_id=workspace_id,
        name="Fixture deleted triage state",
        group="triage",
        is_triage=True,
    )
    archived_project = fixture.create(
        Project,
        "archived",
        workspace_id=workspace_id,
        name="Fixture archived project",
        identifier="fx-arch-proj",
        archived_at=datetime.datetime.now(datetime.UTC),
    )
    archived_project_state = fixture.create(
        State,
        "archived-project-state",
        project_id=archived_project.pk,
        workspace_id=workspace_id,
        name="Fixture archived project state",
    )
    deleted_project = fixture.create(
        Project,
        "deleted-project",
        deleted=True,
        workspace_id=workspace_id,
        name="Fixture deleted project",
        identifier="fx-del-proj",
    )
    deleted_project_state = fixture.create(
        State,
        "deleted-project-state",
        project_id=deleted_project.pk,
        workspace_id=workspace_id,
        name="Fixture deleted project state",
    )

    common = {"project_id": project.pk, "workspace_id": workspace_id}
    fixture.create(
        Issue, "triage", state_id=triage.pk, name="Fixture triage issue", **common
    )
    fixture.create(
        Issue,
        "triage-deleted-state",
        state_id=triage_deleted.pk,
        name="Fixture issue in a deleted triage state",
        **common,
    )
    fixture.create(
        Issue,
        "archived",
        archived_at=datetime.date.today(),
        name="Fixture archived issue",
        **common,
    )
    fixture.create(
        Issue,
        "draft",
        is_draft=True,
        name="Fixture draft issue",
        **common,
    )
    fixture.create(
        Issue,
        "archived-project",
        project_id=archived_project.pk,
        workspace_id=workspace_id,
        state_id=archived_project_state.pk,
        name="Fixture issue in an archived project",
    )
    fixture.create(
        Issue,
        "deleted-project",
        project_id=deleted_project.pk,
        workspace_id=workspace_id,
        state_id=deleted_project_state.pk,
        name="Fixture issue in a deleted project",
    )


def write_golden(models: list[Any]) -> int:
    import django

    entries = []
    for model in models:
        pk_name = model._meta.pk.name
        for manager in model._meta.managers:
            manager_object = getattr(model, manager.name)
            ids = sorted(str(value) for value in manager_object.values_list(pk_name, flat=True))
            entries.append(
                {
                    "model": model._meta.label,
                    "table": model._meta.db_table,
                    "manager": manager.name,
                    "pk_column": model._meta.pk.column,
                    "ids": ids,
                }
            )
    document = {
        "generated_from": {
            "tool": "apps/server/tools/codegen/manager_fixture.py",
            "fixture_marker": f"workspaces.slug = {MARKER_SLUG!r}",
            "django": django.get_version(),
        },
        "counts": {"models": len(models), "managers": len(entries)},
        "managers": entries,
    }
    GOLDEN.parent.mkdir(parents=True, exist_ok=True)
    GOLDEN.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    return len(entries)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--database-url",
        default=os.environ.get("DATABASE_URL"),
        help="the disposable clone to fill (default: DATABASE_URL)",
    )
    arguments = parser.parse_args(argv)

    if not arguments.database_url:
        parser.error("DATABASE_URL or --database-url is required")
    os.environ["DATABASE_URL"] = arguments.database_url

    django = setup_django()
    from django.db import connection

    name = connection.settings_dict["NAME"]
    if not name.startswith("t_"):
        raise SystemExit(
            f"refusing to write to database {name!r}: the fixture only runs on a "
            "t_* clone (apps/server/AGENTS.md)"
        )
    with connection.cursor() as cursor:
        cursor.execute("SELECT 1 FROM workspaces WHERE slug = %s", [MARKER_SLUG])
        if cursor.fetchone():
            raise SystemExit(
                f"database {name!r} already contains the manager fixture; clone a "
                "fresh template with apps/server/dev/mkdb.sh instead"
            )

    import models as codegen

    fixture = Fixture()
    models_list = codegen.concrete_models()
    build_rows(fixture, models_list)
    managers = write_golden(models_list)

    print(
        f"fixture: {fixture.created} rows in {len(models_list)} models; "
        f"wrote {managers} manager id sets to {GOLDEN.relative_to(REPO_ROOT)}"
    )
    print(f"django: {django.get_version()}, database: {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
