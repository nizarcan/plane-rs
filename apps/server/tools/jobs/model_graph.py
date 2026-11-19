#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Dump Django's model graph for the deletion tasks.

`plane/bgtasks/deletion_task.py` walks Django metadata, not hand-written lists:
`soft_delete_related_objects` iterates `instance._meta.get_fields()` and
`hard_delete` deletes querysets whose collector cascades through every reverse
relation. The Rust port needs the same metadata, so this script records it with
the pinned Django (5.2.17) and writes it as JSON that `plane-tasks` embeds.

Usage (from `apps/server/`, with the dev environment exported like the
differential harness does):

  PLANE_DB=t_p8d_graph ../api/.venv/bin/python tools/jobs/model_graph.py \
      --out crates/plane-tasks/data/model_graph.json

The output is deterministic: models are sorted by app label and name, and so
are the relation lists. `--check` fails when the checked-in file is stale.
"""

import argparse
import json
import os
import sys


def collect() -> dict:
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

    import django

    django.setup()

    from django.apps import apps
    from django.db import models

    def on_delete_name(field) -> str:
        behaviour = field.remote_field.on_delete
        # `SET_NULL`/`CASCADE` are functions; `SET(value)` is a closure whose
        # `__name__` is "SET_NULL" for SET_NULL(value=None). Keep the name the
        # Python cascade compares against, and record anything else verbatim.
        return getattr(behaviour, "__name__", repr(behaviour))

    def model_entries():
        entries = []
        for model in apps.get_models():
            if model._meta.app_label not in ("db", "license"):
                continue
            fields = []
            for field in model._meta.concrete_fields:
                fields.append(
                    {
                        "name": field.name,
                        "column": field.attname,
                        "null": bool(field.null),
                        "primary_key": bool(field.primary_key),
                        "kind": field.get_internal_type(),
                    }
                )
            parents = []
            for field in model._meta.concrete_fields:
                if not field.is_relation or field.remote_field is None:
                    continue
                if field.related_model is None:
                    continue
                parents.append(
                    {
                        "column": field.attname,
                        "field": field.name,
                        "parent": field.related_model._meta.label_lower,
                        "on_delete": on_delete_name(field),
                        "null": bool(field.null),
                    }
                )
            children = []
            for relation in model._meta.get_fields():
                # The soft cascade's own filter: reverse relations only.
                if not (relation.one_to_many or relation.one_to_one):
                    continue
                if not relation.auto_created or relation.concrete:
                    continue
                field = relation.field
                children.append(
                    {
                        "model": relation.related_model._meta.label_lower,
                        "field": field.name,
                        "column": field.attname,
                        "accessor": relation.get_accessor_name(),
                        "on_delete": on_delete_name(field),
                        "one_to_one": bool(relation.one_to_one),
                        "null": bool(field.null),
                    }
                )
            parents.sort(key=lambda item: (item["column"], item["parent"]))
            fields.sort(key=lambda item: item["name"])
            base_manager = type(model._base_manager).__name__
            default_manager = (
                type(model._default_manager).__name__ if model._default_manager else None
            )
            managers = []
            for manager in model._meta.managers:
                managers.append(
                    {
                        "name": manager.name,
                        "class": type(manager).__name__,
                        "default": bool(model._meta.default_manager_name == manager.name)
                        or manager is model._meta.default_manager,
                        "base": manager is model._meta.base_manager,
                    }
                )
            managers.sort(key=lambda item: (item["name"], item["class"]))
            entries.append(
                {
                    "label": model._meta.label_lower,
                    "table": model._meta.db_table,
                    "has_deleted_at": any(f.name == "deleted_at" for f in model._meta.concrete_fields),
                    "base_manager": base_manager,
                    "default_manager": default_manager,
                    "managers": managers,
                    "fields": fields,
                    "parents": parents,
                    "children": children,
                }
            )
        return entries

    # `hard_delete`'s final loop iterates `apps.get_models()` in registry order.
    registry_order = [
        model._meta.label_lower
        for model in apps.get_models()
        if model._meta.app_label in ("db", "license")
    ]
    entries = model_entries()
    entries.sort(key=lambda item: (item["label"], item["table"]))
    return {
        "django_version": __import__("django").get_version(),
        "apps": ["db", "license"],
        "registry_order": registry_order,
        "models": entries,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    encoded = json.dumps(collect(), indent=1, sort_keys=True) + "\n"
    if args.check:
        with open(args.out) as handle:
            current = handle.read()
        if current != encoded:
            print(f"{args.out} is stale; regenerate with tools/jobs/model_graph.py", file=sys.stderr)
            return 1
        print(f"{args.out} is current")
        return 0
    with open(args.out, "w") as handle:
        handle.write(encoded)
    print(f"wrote {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
