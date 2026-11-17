#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the serializer inventory from DRF's own introspection.

Django is the specification for the Rust rewrite (see RUST_REWRITE.md). This
script imports every serializer module in ``apps/api/plane``, instantiates every
serializer class the way a view does (no instance, no context), and writes:

* ``apps/server/serializers.json`` -- the machine-readable inventory, including
  every field in DRF's output order with its class, ``source``, null/read-only
  flags, nested serializer, method name, format, choices, pk type, JSON details
  and the Django model field it reads.
* ``apps/server/SERIALIZERS.md`` -- the readable tables: one per serializer,
  plus the view -> serializer index.
* ``apps/server/crates/plane-serializers/src/generated/*.rs`` -- for every
  serializer with a field-by-field ``to_representation``, a Rust struct in
  output order with the wrapper type that renders each field exactly, and a
  ``DrfSerialize`` impl honouring ``fields=``/``expand=``.

Nothing here opens a database or Redis connection: field construction reads
model metadata only. One trap: ``RelatedField.choices`` *does* run
``queryset.exists()``/iteration, so this script never reads ``choices`` from a
related field -- only from ``ChoiceField``.

It expects to run under the already-synced ``apps/api/.venv``::

    cd apps/api
    .venv/bin/python ../server/tools/inventory/serializers.py

The defaults below make the shorter form work too::

    apps/api/.venv/bin/python apps/server/tools/inventory/serializers.py

``--check`` compares the generated files with what is on disk and exits 1 on a
difference (CI freshness). ``--rust`` writes the Rust modules; it is the
default, and exists so the documented invocation
(``tools/inventory/serializers.py --rust``) is explicit. ``--rust-dir DIR``
moves the representation output, ``--skip-rust`` skips all Rust.

The same run also emits the write-path tables `plane-http-app` uses: one
module per serializer under ``crates/plane-http-app/src/write/generated/``,
with the writable fields in DRF validation order, the read-only fields,
``Meta.extra_kwargs`` and a generated ``validate``. ``--write-serializers A,B``
selects the classes (default: the work-items set plus Sticky and Project);
``--write-dir DIR`` moves the output. ``--write-equivalence`` compares the
generated Sticky/Project tables with the hand-written handlers in
``stickies/write.rs`` and ``projects/write.rs``; it is diagnostic, because the
hand-written side may be the one that is wrong.

## Classification

``codegen`` is one of:

``simple``
    ``to_representation`` is DRF's ``Serializer.to_representation``: one key per
    readable field, in declaration order (``rest_framework/serializers.py:530``).
    Generated verbatim.

``dynamic``
    ``to_representation`` is
    ``plane.app.serializers.base.DynamicBaseSerializer``'s: ``fields=`` filters
    output, ``expand=`` injects fields and replaces values
    (``plane/app/serializers/base.py:91-190``). Generated, with
    ``FilterMode::OutputOnly`` and the expansion pass.

``v1``
    ``to_representation`` is
    ``plane.api.serializers.base.BaseSerializer``'s: ``fields=`` removes fields,
    ``expand=`` only replaces values (``plane/api/serializers/base.py:26-141``).
    Generated, with ``FilterMode::RemoveFields``.

``manual``
    Anything else -- a hand-written ``to_representation``, a ``get_fields`` or
    ``__init__`` override outside the two generic bases, or a field whose output
    type cannot be derived from model metadata. These are listed in
    ``SERIALIZERS.md`` with the reason, and get hand-written Rust modules in a
    later session.

## Why the field order is taken from ``_readable_fields``

``Serializer.to_representation`` walks ``_readable_fields``, which is
``self.fields.values()`` minus ``write_only`` fields
(``rest_framework/serializers.py:397-400``). ``self.fields`` is a
``BindingDict`` built from ``get_fields()``, and dicts keep insertion order, so
the order is ``Meta.fields`` (or, for ``fields = "__all__"``,
``[pk] + declared + model fields + forward relations``) after DRF's
``deepcopy`` and any ``__init__`` mutation. Iterating ``_readable_fields`` is
the only way to see post-``__init__`` order (``DynamicBaseSerializer`` appends
injected expansion fields there when instantiated with ``expand=``; this script
records the unexpanded order and documents injection separately).
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import importlib
import inspect
import json
import os
import pkgutil
import re
import subprocess
import sys
import uuid
from collections import OrderedDict
from enum import Enum
from pathlib import Path
from typing import Any

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"

GENERATED_DIR = SERVER_DIR / "crates" / "plane-serializers" / "src" / "generated"

# Every package that defines serializers. Walked recursively.
SERIALIZER_PACKAGES = (
    "plane.app.serializers",
    "plane.api.serializers",
    "plane.space.serializer",
    "plane.license.api.serializers",
)

# `to_representation` owners that the Rust runtime reproduces generically.
DRF_DEFAULT_TO_REPRESENTATION = "rest_framework.serializers.Serializer"
DYNAMIC_TO_REPRESENTATION = "plane.app.serializers.base.DynamicBaseSerializer"
V1_TO_REPRESENTATION = "plane.api.serializers.base.BaseSerializer"

# `__init__`/`get_fields` owners that are generic (their behaviour is modelled
# by `plane-serializers`); any other owner makes the serializer manual.
GENERIC_INIT_OWNERS = frozenset(
    {
        "rest_framework.serializers.BaseSerializer",
        "rest_framework.serializers.Serializer",
        "rest_framework.serializers.ModelSerializer",
        "plane.app.serializers.base.BaseSerializer",
        "plane.app.serializers.base.DynamicBaseSerializer",
        "plane.api.serializers.base.BaseSerializer",
        "plane.space.serializer.base.BaseSerializer",
        "plane.license.api.serializers.base.BaseSerializer",
    }
)
GENERIC_GET_FIELDS_OWNERS = frozenset(
    {
        "rest_framework.serializers.Serializer",
        "rest_framework.serializers.ModelSerializer",
    }
)

# Rust keywords (2015/2018/2021/2024 reserved words). A field named one of these
# is written with the `r#` prefix.
RUST_KEYWORDS = frozenset(
    """
    as break const continue crate else enum extern false fn for if impl in let
    loop match mod move mut pub ref return self Self static struct super trait
    true type unsafe use where while async await dyn abstract become box do
    final macro override priv typeof unsized virtual yield try union gen
    """.split()
)

# A `ChoiceField` with more entries than this is stored once in
# `serializers.json`'s `choice_sets` instead of in every serializer that uses
# it. The timezone list alone is ~600 zones and appears in a dozen serializers.
CHOICES_INLINE_LIMIT = 32

# DRF model-field -> Python type. The wrapper types in `plane-serializers`
# follow from this.
MODEL_FIELD_TYPES: dict[str, str] = {
    "AutoField": "int",
    "BigAutoField": "int",
    "SmallAutoField": "int",
    "IntegerField": "int",
    "BigIntegerField": "int",
    "SmallIntegerField": "int",
    "PositiveIntegerField": "int",
    "PositiveSmallIntegerField": "int",
    "PositiveBigIntegerField": "int",
    "FloatField": "float",
    "BooleanField": "bool",
    "CharField": "str",
    "TextField": "str",
    "SlugField": "str",
    "EmailField": "str",
    "URLField": "str",
    "GenericIPAddressField": "str",
    "FilePathField": "str",
    "FileField": "str",
    "DateTimeField": "datetime",
    "DateField": "date",
    "TimeField": "time",
    "DurationField": "duration",
    "DecimalField": "decimal",
    "BinaryField": "bytes",
    "JSONField": "json",
    "UUIDField": "uuid",
}


# ---------------------------------------------------------------------------
# Django bootstrap
# ---------------------------------------------------------------------------


def setup_django() -> Any:
    """Import Django with production settings, without touching a service."""
    if str(API_DIR) not in sys.path:
        sys.path.insert(0, str(API_DIR))

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "inventory")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

    import django

    django.setup()
    return django


# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------


def dotted(value: Any) -> str:
    module = getattr(value, "__module__", None) or "?"
    qualname = getattr(value, "__qualname__", None) or getattr(value, "__name__", None) or repr(value)
    return f"{module}.{qualname}"


def find_owner(cls: type, attribute: str) -> type | None:
    for klass in cls.__mro__:
        if attribute in vars(klass):
            return klass
    return None


def owner_name(cls: type, attribute: str) -> str | None:
    owner = find_owner(cls, attribute)
    return dotted(owner) if owner is not None else None


def jsonable(value: Any) -> Any:
    """A deterministic JSON-safe view of a Python value."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if isinstance(value, Enum):
        return jsonable(value.value)
    if isinstance(value, (list, tuple)):
        return [jsonable(item) for item in value]
    if isinstance(value, dict):
        return OrderedDict((str(jsonable(key)), jsonable(item)) for key, item in value.items())
    return str(value)


def model_label(model: Any) -> str | None:
    if model is None:
        return None
    return f"{model._meta.app_label}.{model.__name__}"


def relative_source(path: str | Path | None) -> str | None:
    if not path:
        return None
    path = Path(path).resolve()
    # A worktree's `apps/api/.venv` is a symlink into the main checkout, so the
    # resolved path is outside `REPO_ROOT`. The committed inventory uses the
    # logical path; keep it stable across checkouts.
    text = str(path)
    venv = "/apps/api/.venv/"
    if venv in text:
        return "apps/api/.venv/" + text.split(venv, 1)[1]
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return text


def source_line(fn: Any) -> dict[str, Any] | None:
    try:
        path = inspect.getsourcefile(fn)
        _, lineno = inspect.getsourcelines(fn)
    except (TypeError, OSError):
        return None
    return {"file": relative_source(path), "line": lineno}


def git_commit() -> str:
    try:
        return subprocess.run(
            ["git", "-C", str(REPO_ROOT), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


# ---------------------------------------------------------------------------
# Serializer discovery
# ---------------------------------------------------------------------------


def serializer_modules() -> list[str]:
    """Every importable module under the serializer packages, sorted."""
    modules: list[str] = []
    for package_name in SERIALIZER_PACKAGES:
        package = importlib.import_module(package_name)
        modules.append(package.__name__)
        for info in pkgutil.walk_packages(package.__path__, prefix=package.__name__ + "."):
            if info.ispkg or info.name.endswith("__pycache__"):
                continue
            modules.append(info.name)
    return sorted(set(modules))


def discover_serializers(serializers_module: Any) -> dict[str, type]:
    """Every distinct ``BaseSerializer`` subclass defined by Plane."""
    classes: dict[str, type] = {}
    for module_name in serializer_modules():
        try:
            module = importlib.import_module(module_name)
        except Exception as exc:  # pragma: no cover - import errors are fatal
            raise RuntimeError(f"cannot import {module_name}: {exc}") from exc
        for _, value in vars(module).items():
            if not inspect.isclass(value):
                continue
            if not issubclass(value, serializers_module.BaseSerializer):
                continue
            if not value.__module__.startswith("plane."):
                continue
            canonical = dotted(value)
            classes.setdefault(canonical, value)
    return dict(sorted(classes.items()))


# ---------------------------------------------------------------------------
# Model field resolution
# ---------------------------------------------------------------------------


def resolve_attribute(model: Any, source_attrs: list[str]) -> tuple[Any, str, list[Any]]:
    """Walk ``source`` like DRF's ``get_attribute`` and describe the endpoint.

    Returns ``(model_field_or_None, kind, path)`` where ``kind`` is one of
    ``model_field``, ``property``, ``annotation``, ``no_model``.
    ``path`` is the list of resolved model fields along the way (relations
    included), useful for pk-type inference.
    """
    if model is None:
        return None, "no_model", []

    current = model
    path: list[Any] = []
    for index, attr in enumerate(source_attrs):
        try:
            field = current._meta.get_field(attr)
        except Exception:
            field = None
        if field is None and attr.endswith("_id"):
            try:
                field = current._meta.get_field(attr[:-3])
            except Exception:
                field = None
        if field is None:
            kind = "property" if hasattr(current, attr) else "annotation"
            # A property/annotation can only be the last attribute.
            return None, kind, path
        path.append(field)
        if field.is_relation and field.related_model is not None and index != len(source_attrs) - 1:
            current = field.related_model
            continue
        return field, "model_field", path

    return None, "annotation", path


def pk_python_type(model: Any) -> str | None:
    if model is None:
        return None
    pk = model._meta.pk
    return MODEL_FIELD_TYPES.get(type(pk).__name__)


def model_field_record(field: Any) -> OrderedDict[str, Any]:
    record: OrderedDict[str, Any] = OrderedDict()
    record["model"] = model_label(field.model)
    record["name"] = field.name
    record["class"] = type(field).__name__
    record["null"] = bool(getattr(field, "null", False))
    record["blank"] = bool(getattr(field, "blank", False))
    record["editable"] = bool(getattr(field, "editable", True))
    record["primary_key"] = bool(getattr(field, "primary_key", False))
    record["concrete"] = bool(getattr(field, "concrete", False))
    if getattr(field, "is_relation", False):
        record["related_model"] = model_label(field.related_model)
        record["many"] = bool(field.many_to_many)
        record["through"] = getattr(getattr(field, "remote_field", None), "through", None) is not None
    max_length = getattr(field, "max_length", None)
    if max_length is not None:
        record["max_length"] = max_length
    if type(field).__name__ == "DecimalField":
        record["max_digits"] = field.max_digits
        record["decimal_places"] = field.decimal_places
    if type(field).__name__ == "ArrayField":
        record["base_field"] = type(field.base_field).__name__
    if type(field).__name__ == "JSONField":
        record["encoder"] = dotted(field.encoder) if field.encoder else None
        record["decoder"] = dotted(field.decoder) if field.decoder else None
    return record


def python_type_for_model_field(field: Any) -> str | None:
    name = type(field).__name__
    if name == "ArrayField":
        base = python_type_for_model_field(field.base_field)
        return f"list<{base}>" if base else None
    if getattr(field, "is_relation", False) and field.related_model is not None:
        target = pk_python_type(field.related_model)
        return f"list<{target}>" if field.many_to_many and target else target
    return MODEL_FIELD_TYPES.get(name)


# ---------------------------------------------------------------------------
# Field records
# ---------------------------------------------------------------------------


def field_record(
    field: Any,
    serializer_cls: type,
    *,
    name: str | None = None,
    depth: int = 0,
    related_model_hint: Any = None,
    base_field_hint: Any = None,
) -> OrderedDict[str, Any]:
    """Describe one DRF field.

    ``depth`` is only used for `child` records, which do not resolve against
    the model (their attribute is a list element). ``related_model_hint`` and
    ``base_field_hint`` carry the parent's resolved model relationship down to
    a `ManyRelatedField`/`ListField` child, which has no `source` of its own.
    """
    from rest_framework import serializers as drf
    from rest_framework import fields as drf_fields
    from rest_framework import relations as drf_relations
    from rest_framework import serializers as drf_serializers

    record: OrderedDict[str, Any] = OrderedDict()
    field_name = name or getattr(field, "field_name", None) or "?"
    record["name"] = field_name
    record["class"] = type(field).__name__
    record["dotted_class"] = dotted(type(field))
    record["source"] = field.source
    record["source_attrs"] = list(field.source_attrs)
    record["read_only"] = bool(field.read_only)
    record["write_only"] = bool(field.write_only)
    record["required"] = bool(field.required)
    record["allow_null"] = bool(field.allow_null)
    record["many"] = False

    if field.default is not drf_fields.empty:
        record["has_default"] = True
        record["default"] = jsonable(field.default)
    allow_blank = getattr(field, "allow_blank", None)
    if allow_blank is not None:
        record["allow_blank"] = bool(allow_blank)
    max_length = getattr(field, "max_length", None)
    if max_length is not None:
        record["max_length"] = max_length

    # Resolve the model field first: the child of a ManyRelatedField/ListField
    # inherits its element type from it.
    model = None
    model_field = None
    kind = None
    path: list[Any] = []
    if depth == 0:
        model = getattr(getattr(serializer_cls, "Meta", None), "model", None)
        model_field, kind, path = resolve_attribute(model, list(field.source_attrs))
        if model_field is not None:
            record["model_field"] = model_field_record(model_field)
        record["attribute"] = kind
    else:
        record["attribute"] = None

    # --- nested serializer -------------------------------------------------
    if isinstance(field, drf.ListSerializer):
        record["many"] = True
        record["nested"] = dotted(type(field.child))
        record["nested_many"] = True
    elif isinstance(field, drf.BaseSerializer):
        record["nested"] = dotted(type(field))
        record["nested_many"] = False
    elif isinstance(field, drf.ManyRelatedField):
        record["many"] = True
        record["nested"] = None
        record["nested_many"] = False
        child_related = None
        if related_model_hint is not None:
            child_related = related_model_hint
        elif model_field is not None and getattr(model_field, "is_relation", False):
            child_related = model_field.related_model
        elif path and getattr(path[-1], "related_model", None) is not None:
            child_related = path[-1].related_model
        record["child"] = field_record(
            field.child_relation,
            serializer_cls,
            name="child",
            depth=depth + 1,
            related_model_hint=child_related,
        )
        related_model = getattr(getattr(field.child_relation, "queryset", None), "model", None)
        if related_model is None:
            related_model = child_related
        record["pk"] = OrderedDict(
            [
                ("related_model", model_label(related_model)),
                ("python_type", pk_python_type(related_model)),
            ]
        )
    elif isinstance(field, drf.ListField):
        record["many"] = True
        base_hint = base_field_hint
        if base_hint is None and model_field is not None and type(model_field).__name__ == "ArrayField":
            base_hint = model_field.base_field
        record["child"] = field_record(
            field.child,
            serializer_cls,
            name="child",
            depth=depth + 1,
            base_field_hint=base_hint,
        )

    # --- SerializerMethodField ---------------------------------------------
    if isinstance(field, drf.SerializerMethodField):
        method_name = getattr(field, "method_name", None) or f"get_{field_name}"
        record["method_name"] = method_name
        record["method_defined"] = callable(getattr(serializer_cls, method_name, None))

    # --- DateTimeField / DateField / TimeField ------------------------------
    if isinstance(field, drf.DateTimeField):
        record["format"] = getattr(field, "format", None) or "iso-8601"
        record["format_explicit"] = "format" in vars(field)
        if hasattr(field, "timezone"):
            timezone = field.timezone
            record["timezone"] = str(timezone) if timezone is not None else None
    if isinstance(field, (drf.DateField, drf.TimeField)) and not isinstance(field, drf.DateTimeField):
        record["format"] = getattr(field, "format", None) or "iso-8601"
        record["format_explicit"] = "format" in vars(field)

    # --- DecimalField --------------------------------------------------------
    if isinstance(field, drf.DecimalField):
        from rest_framework.settings import api_settings

        record["decimal"] = OrderedDict(
            [
                ("max_digits", field.max_digits),
                ("decimal_places", field.decimal_places),
                (
                    "coerce_to_string",
                    getattr(field, "coerce_to_string", api_settings.COERCE_DECIMAL_TO_STRING),
                ),
                ("rounding", str(field.rounding) if field.rounding is not None else None),
                ("normalize_output", bool(field.normalize_output)),
            ]
        )

    # --- ChoiceField ---------------------------------------------------------
    if isinstance(field, drf.ChoiceField) and not isinstance(field, drf.MultipleChoiceField):
        # Never touch `.choices` on a related field: RelatedField.choices runs
        # a queryset, which needs a database.
        record["choices"] = [
            OrderedDict([("value", jsonable(value)), ("display", jsonable(display))])
            for value, display in field.choices.items()
        ]

    # --- JSONField -----------------------------------------------------------
    if isinstance(field, drf.JSONField):
        record["json"] = OrderedDict(
            [
                ("encoder", dotted(field.encoder) if field.encoder else None),
                ("decoder", dotted(field.decoder) if field.decoder else None),
                ("binary", bool(field.binary)),
            ]
        )

    # --- PrimaryKeyRelatedField ---------------------------------------------
    related_model = None
    if isinstance(field, (drf.PrimaryKeyRelatedField, drf.ManyRelatedField)):
        pk_field = getattr(field, "pk_field", None)
        queryset = getattr(field, "queryset", None)
        related_model = getattr(queryset, "model", None)
        record["pk_field"] = dotted(type(pk_field)) if pk_field is not None else None

    # --- output type ---------------------------------------------------------
    if kind == "model_field" and model_field is not None:
        python_type = python_type_for_model_field(model_field)
    else:
        python_type = None
    if python_type is None and kind == "property" and depth == 0:
        python_type = property_python_type(model, field.source_attrs[0])
    if python_type is None:
        python_type = python_type_for_field_class(field)
    if python_type is None and related_model_hint is not None:
        python_type = pk_python_type(related_model_hint)
    if python_type is None and base_field_hint is not None:
        python_type = python_type_for_model_field(base_field_hint)

    if isinstance(field, drf.SerializerMethodField):
        # The method's return shape is Python code, not model metadata. The
        # generated field is a `DrfValue`, which renders any JSON shape.
        python_type = "method"
    elif record.get("nested"):
        # The nested serializer decides the shape, not the model relation: the
        # `Type` column says what the JSON value is, not what it was read from.
        python_type = (
            "list<serializer>" if record.get("nested_many") else "serializer"
        )
    elif python_type is None and record.get("nested"):
        python_type = "serializer"
    elif python_type is None and isinstance(field, drf.PrimaryKeyRelatedField):
        # A read-only declared field with no queryset over a view annotation
        # (for example `IssueSerializer.cycle_id`, annotated by a Subquery).
        # The pk can be a UUID, an integer or a string; `DrfPk` holds any.
        python_type = "pk"
    elif python_type is None and isinstance(field, drf.ReadOnlyField):
        # `ReadOnlyField` returns the raw attribute. Over an annotation the type
        # is unknowable from metadata; `DrfValue` renders whatever the handler
        # supplies.
        python_type = "value"
    record["python_type"] = python_type

    if related_model is None and isinstance(field, drf.PrimaryKeyRelatedField):
        if model_field is not None and getattr(model_field, "is_relation", False):
            related_model = model_field.related_model
        elif path and getattr(path[-1], "related_model", None) is not None:
            related_model = path[-1].related_model
    if isinstance(field, drf.PrimaryKeyRelatedField):
        record["pk"] = OrderedDict(
            [
                ("related_model", model_label(related_model)),
                ("python_type", pk_python_type(related_model)),
            ]
        )

    record["skippable"] = bool(
        not isinstance(field, drf.SerializerMethodField)
        and not record.get("nested")
        and not field.required
        and not field.allow_null
        and record.get("attribute") == "annotation"
    )

    return record


def property_python_type(model: Any, name: str) -> str | None:
    """Python type of the model properties Plane serialises.

    A property has no annotation, so these are read from the source. Only the
    handful Plane uses as `ReadOnlyField` are listed; anything else makes the
    serializer `manual` (the generator refuses to guess).
    """
    if model is None:
        return None
    known = {
        "avatar_url": "str",
        "cover_image_url": "str",
        "logo_url": "str",
        "asset_url": "str",
        "status": "str",
        "full_name": "str",
    }
    if name in known:
        return known[name]
    return None


def python_type_for_field_class(field: Any) -> str | None:
    from rest_framework import serializers as drf

    if isinstance(field, drf.UUIDField):
        return "uuid"
    if isinstance(field, drf.IntegerField):
        return "int"
    if isinstance(field, drf.FloatField):
        return "float"
    if isinstance(field, drf.DecimalField):
        return "decimal"
    if isinstance(field, drf.BooleanField):
        return "bool"
    if isinstance(field, drf.DateTimeField):
        return "datetime"
    if isinstance(field, drf.DateField):
        return "date"
    if isinstance(field, drf.TimeField):
        return "time"
    if isinstance(field, drf.DecimalField):
        return "decimal"
    if isinstance(field, drf.JSONField):
        return "json"
    if isinstance(field, drf.PrimaryKeyRelatedField):
        related = getattr(getattr(field, "queryset", None), "model", None)
        return pk_python_type(related)
    if isinstance(field, drf.ListField):
        child = python_type_for_field_class(field.child)
        return f"list<{child}>" if child else None
    if isinstance(
        field,
        (
            drf.CharField,
            drf.EmailField,
            drf.SlugField,
            drf.URLField,
            drf.FileField,
            drf.ReadOnlyField,
        ),
    ):
        return "str"
    if isinstance(field, drf.ChoiceField):
        values = list(field.choices.values())
        if values and all(isinstance(value, bool) for value in values):
            return "bool"
        if values and all(isinstance(value, int) and not isinstance(value, bool) for value in values):
            return "int"
        return "str"
    return None


# ---------------------------------------------------------------------------
# Serializer records
# ---------------------------------------------------------------------------


def codegen_kind(cls: type, fields: list[OrderedDict[str, Any]]) -> tuple[str, str | None]:
    """``(kind, manual_reason)`` for a serializer class."""
    to_representation = owner_name(cls, "to_representation")
    if to_representation == DRF_DEFAULT_TO_REPRESENTATION:
        kind = "simple"
    elif to_representation == DYNAMIC_TO_REPRESENTATION:
        kind = "dynamic"
    elif to_representation == V1_TO_REPRESENTATION:
        kind = "v1"
    else:
        return "manual", f"to_representation defined in {to_representation}"

    init_owner = owner_name(cls, "__init__")
    if init_owner is not None and init_owner not in GENERIC_INIT_OWNERS:
        return "manual", f"__init__ defined in {init_owner}"
    get_fields_owner = owner_name(cls, "get_fields")
    if get_fields_owner is not None and get_fields_owner not in GENERIC_GET_FIELDS_OWNERS:
        return "manual", f"get_fields defined in {get_fields_owner}"

    for field in fields:
        if field.get("python_type") is None and not field.get("nested"):
            return "manual", f"field {field['name']!r} has no derivable output type"
        child = field.get("child")
        if (
            child is not None
            and child.get("python_type") is None
            and not child.get("nested")
        ):
            return "manual", f"field {field['name']!r} child has no derivable output type"
    return kind, None
def serializer_record(cls: type, canonical: str) -> OrderedDict[str, Any]:
    record: OrderedDict[str, Any] = OrderedDict()
    record["name"] = canonical
    record["module"] = cls.__module__
    record["class"] = cls.__name__
    base = next((parent for parent in cls.__mro__[1:] if parent is not object), None)
    record["base"] = dotted(base) if base is not None else None
    model = getattr(getattr(cls, "Meta", None), "model", None)
    record["model"] = model_label(model)

    abstract = False
    error: str | None = None
    fields: list[OrderedDict[str, Any]] = []
    write_only: list[OrderedDict[str, Any]] = []
    try:
        instance = cls()
        readable = list(instance._readable_fields)
        all_fields = list(instance.fields.values())
    except Exception as exc:
        abstract = True
        message = str(exc)
        if 'missing "Meta' in message:
            error = "abstract base class: no `Meta.model`, not instantiable"
        elif isinstance(exc, AttributeError) and "'list' object has no attribute 'values'" in message:
            error = (
                "`__init__` replaces `self.fields` with a list; "
                "`to_representation` is hand-written"
            )
        else:
            error = f"{type(exc).__name__}: {message}"
        readable = []
        all_fields = []

    if abstract:
        record["abstract"] = True
        record["abstract_reason"] = error
    else:
        record["abstract"] = False
        for field in readable:
            fields.append(field_record(field, cls))
        for field in all_fields:
            if field.write_only:
                write_only.append(field_record(field, cls))
        if not fields and isinstance(vars(instance).get("fields"), list):
            abstract = True
            record["abstract"] = True
            record["abstract_reason"] = "self.fields is replaced with a list in __init__"

    record["overrides"] = OrderedDict(
        [
            ("to_representation", owner_name(cls, "to_representation")),
            ("get_fields", owner_name(cls, "get_fields")),
            ("__init__", owner_name(cls, "__init__")),
            ("create", owner_name(cls, "create")),
            ("update", owner_name(cls, "update")),
            (
                "validate",
                sorted(
                    name
                    for name in dir(cls)
                    if (name == "validate" or name.startswith("validate_"))
                    and callable(getattr(cls, name, None))
                    and (owner := find_owner(cls, name)) is not None
                    and owner.__module__.startswith("plane.")
                ),
            ),
        ]
    )

    if abstract:
        kind, reason = "manual", record["abstract_reason"]
    else:
        kind, reason = codegen_kind(cls, fields)
    record["codegen"] = kind
    if reason is not None:
        record["manual_reason"] = reason
    record["fields"] = fields
    record["write_only_fields"] = write_only

    readable_names = [field["name"] for field in fields]
    # Expansion behaviour, from `get_expansion_mapper()` and the many set.
    from plane.app.serializers.base import MANY_EXPANSION_FIELDS, get_expansion_mapper

    mapper = get_expansion_mapper()
    record["expansion"] = OrderedDict(
        [
            ("mode", {"dynamic": "inject", "v1": "replace-only", "simple": "none"}.get(kind, "none")),
            (
                "existing_fields",
                [name for name in mapper if name in readable_names],
            ),
            (
                "injectable_fields",
                [name for name in mapper if name not in readable_names],
            ),
            ("many_fields", sorted(MANY_EXPANSION_FIELDS)),
            ("issue_attachments", kind in ("dynamic",)),
        ]
    )
    record["field_count"] = len(fields)
    return record


def dedupe_choice_sets(serializers: list[dict[str, Any]]) -> OrderedDict[str, Any]:
    """Move large `ChoiceField` lists into one shared table.

    Every `choices` record stays in the JSON -- either inline (small lists) or
    as `{"ref": ..., "count": ...}` pointing at [`choice_sets`]. The reference
    is `choices:<sha1 prefix>` of the list itself, so identical lists (the
    timezone choices on `timezone` and `user_timezone`) share one entry.
    """
    sets: dict[str, list[OrderedDict[str, Any]]] = {}

    def visit(field: OrderedDict[str, Any]) -> None:
        choices = field.get("choices")
        if choices is not None and len(choices) > CHOICES_INLINE_LIMIT:
            canonical = json.dumps(choices, sort_keys=True, ensure_ascii=False)
            digest = hashlib.sha1(canonical.encode("utf-8")).hexdigest()[:12]
            key = f"choices:{digest}"
            sets.setdefault(key, choices)
            field.pop("choices")
            field["choices_ref"] = key
            field["choices_count"] = len(choices)
        child = field.get("child")
        if child is not None:
            visit(child)

    for record in serializers:
        for field in record["fields"] + record["write_only_fields"]:
            visit(field)
    return OrderedDict((key, sets[key]) for key in sorted(sets))


def expansion_mapper_record() -> OrderedDict[str, Any]:
    from plane.app.serializers.base import MANY_EXPANSION_FIELDS, get_expansion_mapper

    mapper = get_expansion_mapper()
    return OrderedDict(
        [
            (
                "mapping",
                OrderedDict((name, dotted(serializer)) for name, serializer in mapper.items()),
            ),
            ("many", sorted(MANY_EXPANSION_FIELDS)),
            (
                "issue_attachments",
                OrderedDict(
                    [
                        ("in_mapper", False),
                        ("appended", "last, when expand= asks for it (DynamicBaseSerializer only)"),
                        ("source", "FileAsset.objects.filter(issue_id=..., entity_type=ISSUE_ATTACHMENT)"),
                    ]
                ),
            ),
        ]
    )


# ---------------------------------------------------------------------------
# Views -> serializers
# ---------------------------------------------------------------------------


def callable_serializer_name(node: ast.AST) -> str | None:
    """The serializer name in a call, if the callee is named ``*Serializer``."""
    name: str | None = None
    if isinstance(node, ast.Name):
        name = node.id
    elif isinstance(node, ast.Attribute):
        name = node.attr
    if name and name.endswith("Serializer"):
        return name
    return None


def call_record(node: ast.Call, view_class: type, method: str) -> OrderedDict[str, Any]:
    name = callable_serializer_name(node.func)
    record: OrderedDict[str, Any] = OrderedDict()
    record["serializer"] = name
    record["method"] = method
    record["file"] = relative_source(inspect.getsourcefile(view_class))
    record["line"] = node.lineno
    kwargs: OrderedDict[str, Any] = OrderedDict()
    for keyword in node.keywords:
        if keyword.arg in ("fields", "expand", "many"):
            try:
                value = ast.unparse(keyword.value)
            except Exception:  # pragma: no cover - ast.unparse is reliable
                value = "<expression>"
            kwargs[keyword.arg] = value
    record["kwargs"] = kwargs
    record["fields_arg"] = "fields" in kwargs
    record["expand_arg"] = "expand" in kwargs
    return record


def class_ast_node(tree: ast.Module, cls: type) -> ast.ClassDef | None:
    qualname = cls.__qualname__.split(".")
    nodes: list[ast.AST] = list(tree.body)
    found: ast.ClassDef | None = None
    for part in qualname:
        found = None
        for node in nodes:
            if isinstance(node, ast.ClassDef) and node.name == part:
                found = node
                break
        if found is None:
            return None
        nodes = list(found.body)
    return found


def module_ast(module_name: str) -> ast.Module | None:
    try:
        source_file = inspect.getsourcefile(importlib.import_module(module_name))
        if not source_file:
            return None
        return ast.parse(Path(source_file).read_text(encoding="utf-8"), filename=source_file)
    except (OSError, SyntaxError):
        return None


def module_functions(tree: ast.Module) -> dict[str, ast.FunctionDef]:
    return {
        node.name: node
        for node in tree.body
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))
    }


def calls_in(node: ast.AST, view_class: type, method: str) -> list[OrderedDict[str, Any]]:
    records = []
    for child in ast.walk(node):
        if isinstance(child, ast.Call) and callable_serializer_name(child.func):
            records.append(call_record(child, view_class, method))
    return records


def view_record(view_path: str, route_paths: list[str]) -> OrderedDict[str, Any]:
    record: OrderedDict[str, Any] = OrderedDict()
    record["view"] = view_path
    record["paths"] = sorted(set(route_paths))
    try:
        module_name, _, class_name = view_path.rpartition(".")
        view_class = getattr(importlib.import_module(module_name), class_name)
    except Exception as exc:
        record["error"] = f"{type(exc).__name__}: {exc}"
        return record

    record["file"] = relative_source(inspect.getsourcefile(view_class))
    if not inspect.isclass(view_class):
        # A plain Django function view: no DRF serializer attributes, but it may
        # still build one by hand.
        record["kind"] = "function"
        record["serializer_class"] = None
        record["get_serializer_class"] = None
        tree = module_ast(view_class.__module__)
        calls: list[OrderedDict[str, Any]] = []
        if tree is not None:
            for node in tree.body:
                if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name == class_name:
                    calls.extend(calls_in(node, view_class, class_name))
        calls.sort(key=lambda item: (item["line"], item["serializer"], item["method"]))
        record["calls"] = calls
        return record
    record["kind"] = "class"
    serializer_class = getattr(view_class, "serializer_class", None)
    if serializer_class is not None:
        record["serializer_class"] = dotted(serializer_class)
        record["serializer_class_defined_in"] = owner_name(view_class, "serializer_class")
    else:
        record["serializer_class"] = None

    get_serializer_class = getattr(view_class, "get_serializer_class", None)
    defined_in = owner_name(view_class, "get_serializer_class")
    if get_serializer_class is not None and defined_in and not defined_in.startswith(
        "rest_framework.generics"
    ):
        entry: OrderedDict[str, Any] = OrderedDict()
        entry["defined_in"] = defined_in
        location = source_line(get_serializer_class)
        if location:
            entry["file"] = location["file"]
            entry["line"] = location["line"]
        # The method may live on a Plane base class; scan the class that
        # defines it, not the concrete view.
        owner = find_owner(view_class, "get_serializer_class") or view_class
        tree = module_ast(owner.__module__)
        returns: list[str] = []
        if tree is not None:
            node = class_ast_node(tree, owner)
            if node is not None:
                for child in ast.walk(node):
                    if isinstance(child, ast.FunctionDef) and child.name == "get_serializer_class":
                        for sub in ast.walk(child):
                            if isinstance(sub, ast.Return):
                                for leaf in ast.walk(sub):
                                    if isinstance(leaf, ast.Name) and leaf.id.endswith("Serializer"):
                                        returns.append(leaf.id)
                                    if isinstance(leaf, ast.Attribute) and leaf.attr == "serializer_class":
                                        returns.append("self.serializer_class")
        entry["returns"] = sorted(set(returns))
        record["get_serializer_class"] = entry
    else:
        record["get_serializer_class"] = None

    tree = module_ast(view_class.__module__)
    calls: list[OrderedDict[str, Any]] = []
    if tree is not None:
        node = class_ast_node(tree, view_class)
        if node is not None:
            helpers = module_functions(tree)
            visited_helpers: set[str] = set()
            for child in node.body:
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    method_calls = calls_in(child, view_class, child.name)
                    calls.extend(method_calls)
                    # One level of local helper functions: `get_issue_serializer`
                    # style wrappers live in the same module.
                    for nested in ast.walk(child):
                        if isinstance(nested, ast.Call) and isinstance(nested.func, ast.Name):
                            helper = helpers.get(nested.func.id)
                            if helper is not None and helper.name not in visited_helpers:
                                visited_helpers.add(helper.name)
                                calls.extend(
                                    calls_in(helper, view_class, f"helper:{helper.name}")
                                )
    calls.sort(key=lambda item: (item["line"], item["serializer"], item["method"]))
    record["calls"] = calls
    return record


def views_index(routes: list[dict[str, Any]]) -> list[OrderedDict[str, Any]]:
    by_view: dict[str, list[str]] = {}
    for route in routes:
        by_view.setdefault(route["view"], []).append(route["path"])
    return [view_record(view, paths) for view, paths in sorted(by_view.items())]


# ---------------------------------------------------------------------------
# Rust codegen
# ---------------------------------------------------------------------------


def rust_module_name(python_module: str) -> str:
    parts = python_module.split(".")
    if parts and parts[0] == "plane":
        parts = parts[1:]
    parts = [part for part in parts if part not in ("serializers", "serializer")]
    return "_".join(parts)


def rust_field_ident(name: str) -> str:
    """A Rust field identifier for a DRF field name.

    DRF field names are not Rust identifiers (`project__identifier` is the
    legal Python name `project__identifier`; `type` is a keyword). The key
    written to JSON is always the original name; only the struct field is
    renamed. `self`/`Self`/`super`/`crate` cannot be raw identifiers.
    """
    if name in ("self", "Self", "super", "crate"):
        return f"{name}_"
    collapsed = re.sub(r"_{2,}", "_", name)
    if collapsed in RUST_KEYWORDS:
        return f"r#{collapsed}"
    return collapsed


def rust_struct_name(record: dict[str, Any]) -> str:
    return record["class"]


def nested_record(record: dict[str, Any], name: str) -> tuple[dict[str, Any], bool] | None:
    for field in record["fields"]:
        if field["name"] == name:
            return field, True
    for field in record["write_only_fields"]:
        if field["name"] == name:
            return field, False
    return None


class RustGenerator:
    def __init__(self, serializers: list[dict[str, Any]]) -> None:
        self.serializers = {record["name"]: record for record in serializers}
        self.idents: dict[int, dict[str, str]] = {}
        # Nested fields that name a manual serializer are handled through
        # `&dyn DrfSerialize`, so the parent still compiles -- except a
        # to-many manual child, whose slice of trait objects has no renderer.
        for record in serializers:
            for field in record["fields"]:
                if field.get("nested") is None or field.get("nested") in self.serializers:
                    continue
                record["manual_reason"] = record.get("manual_reason") or (
                    f"nested serializer {field['nested']} is not generated"
                )
                if field.get("nested_many"):
                    record["codegen"] = "manual"

    def struct_lifetime(self, record: dict[str, Any]) -> bool:
        """Whether the generated struct borrows (and therefore has `<'a>`)."""
        for field in record["fields"]:
            kind = field["python_type"]
            if field.get("nested") or field.get("many"):
                return True
            if kind in ("str", "json", "bytes"):
                return True
            if field["class"] == "SerializerMethodField":
                return True
        return False

    def rust_type_path(self, canonical: str) -> tuple[str, bool]:
        record = self.serializers[canonical]
        module = rust_module_name(record["module"])
        name = record["class"]
        has_lifetime = self.struct_lifetime(record)
        path = f"crate::generated::{module}::{name}"
        if has_lifetime:
            path += "<'a>"
        return path, has_lifetime

    def element_kind(self, field: dict[str, Any]) -> tuple[str | None, bool]:
        """``(element type, is a list)`` for a field."""
        kind = field["python_type"]
        if kind.startswith("list<") and kind.endswith(">"):
            return kind[5:-1], True
        return kind, bool(field.get("many"))

    def field_type(self, field: dict[str, Any]) -> tuple[str, bool]:
        """``(Rust type, borrows)`` for one field."""
        name = field["class"]

        if field.get("nested"):
            nested = field["nested"]
            if nested in self.serializers:
                path, _ = self.rust_type_path(nested)
                if field.get("nested_many"):
                    return f"&'a [{path}]", True
                return f"Option<&'a {path}>", True
            if field.get("nested_many"):
                return "&'a [&'a (dyn crate::ser::DrfSerialize + 'a)]", True
            return "Option<&'a (dyn crate::ser::DrfSerialize + 'a)>", True

        kind, many = self.element_kind(field)
        many_optional = many and bool(field.get("allow_null"))
        if self.skippable(field):
            # A skippable field is three-state (absent / null / value); see
            # `plane_serializers::fields::FieldValue`.
            return (
                f"crate::fields::FieldValue<{self.scalar_type(field, kind, many)}>",
                True,
            )
        if kind in ("method", "value"):
            return "crate::value::DrfValue<'a>", True
        if kind == "pk":
            return "crate::fields::DrfPk<'a>", True
        optional = self.optional(field)
        skippable = self.skippable(field)
        nullable = optional or skippable
        if kind == "json":
            return "Option<crate::fields::PgJson<'a>>", True
        if kind == "bytes":
            return "Option<&'a [u8]>", True
        if kind == "uuid":
            if many:
                return (
                    "Option<&'a [uuid::Uuid]>"
                    if skippable or many_optional
                    else "&'a [uuid::Uuid]"
                ), True
            return ("Option<uuid::Uuid>" if nullable else "uuid::Uuid"), nullable
        if kind == "str":
            if many:
                return (
                    "Option<&'a [&'a str]>"
                    if skippable or many_optional
                    else "&'a [&'a str]"
                ), True
            return ("Option<&'a str>" if nullable else "&'a str"), True
        if kind == "int":
            if many:
                return (
                    "Option<&'a [i64]>" if skippable or many_optional else "&'a [i64]"
                ), True
            return ("Option<i64>" if nullable else "i64"), nullable
        if kind == "float":
            if many:
                return (
                    "Option<&'a [f64]>" if skippable or many_optional else "&'a [f64]"
                ), True
            return ("Option<f64>" if nullable else "f64"), nullable
        if kind == "bool":
            if many:
                return (
                    "Option<&'a [bool]>" if skippable or many_optional else "&'a [bool]"
                ), True
            return ("Option<bool>" if nullable else "bool"), nullable
        if kind == "datetime":
            return (
                "Option<chrono::DateTime<chrono::Utc>>"
                if nullable
                else "chrono::DateTime<chrono::Utc>"
            ), nullable
        if kind == "date":
            return ("Option<chrono::NaiveDate>" if nullable else "chrono::NaiveDate"), nullable
        if kind == "time":
            return ("Option<chrono::NaiveTime>" if nullable else "chrono::NaiveTime"), nullable
        if kind == "decimal":
            return (
                "Option<crate::fields::PyDecimal>"
                if nullable
                else "crate::fields::PyDecimal"
            ), nullable
        raise SystemExit(f"unhandled python type {kind!r} for {name}")

    def scalar_type(self, field: dict[str, Any], kind: str, many: bool) -> str:
        """The inner type of a skippable field's `FieldValue<T>`.

        The wrapper a field class renders through, without the `Option` that
        used to stand for "skip". `FieldValue` supplies the null/absent cases,
        so the inner type is always the present-value type.
        """
        if kind in ("method", "value"):
            return "crate::value::DrfValue<'a>"
        if kind == "pk":
            return "crate::fields::DrfPk<'a>"
        if kind == "json":
            return "crate::fields::PgJson<'a>"
        if kind == "bytes":
            return "&'a [u8]"
        if many:
            return {
                "uuid": "&'a [uuid::Uuid]",
                "str": "&'a [&'a str]",
                "int": "&'a [i64]",
                "float": "&'a [f64]",
                "bool": "&'a [bool]",
            }[kind]
        return {
            "uuid": "uuid::Uuid",
            "str": "&'a str",
            "int": "i64",
            "float": "f64",
            "bool": "bool",
            "datetime": "chrono::DateTime<chrono::Utc>",
            "date": "chrono::NaiveDate",
            "time": "chrono::NaiveTime",
            "decimal": "crate::fields::DrfDecimal",
        }[kind]

    def optional(self, field: dict[str, Any]) -> bool:
        """Whether the value can be `None` on the model."""
        model_field = field.get("model_field")
        if model_field is None:
            # A property returns None in Plane for missing images.
            return field.get("attribute") == "property"
        return bool(model_field.get("null", False))

    def skippable(self, field: dict[str, Any]) -> bool:
        """Whether DRF can omit the key entirely.

        A declared field with `required=False` whose attribute is a view
        annotation (so it may not exist on the instance) goes through
        `Field.get_attribute` -> `AttributeError` -> `SkipField`
        (`rest_framework/fields.py:421-446`), and `to_representation` does not
        write the key (`serializers.py:533-551`). `StateSerializer.order` is the
        visible case: `state/list` adds it to the rendered dict afterwards, so
        the retrieve response has no `"order"` key at all
        (`plane/app/views/state/base.py:102-104`).

        The field is generated as `FieldValue<T>`: `Absent` omits the key,
        `Null` writes `null` (the attribute exists and is `None`, which DRF
        writes as `null` without calling the field), and `Value` renders it.
        A nullable *model column* stays `Option<T>` because its attribute
        always exists; the two are never mixed because `allow_null` fields are
        not skippable.
        """
        if field.get("nested"):
            return False
        if field["class"] == "SerializerMethodField":
            return False
        if field.get("required", True) or field.get("allow_null"):
            return False
        return field.get("attribute") == "annotation"

    def ident(self, record: dict[str, Any], field: dict[str, Any]) -> str:
        """The struct field name for a field, unique within the struct."""
        idents = self.idents.setdefault(id(record), {})
        if field["name"] in idents:
            return idents[field["name"]]
        base = rust_field_ident(field["name"])
        candidate = base
        suffix = 2
        existing = set(idents.values())
        while candidate in existing:
            candidate = f"{base}_{suffix}"
            suffix += 1
        idents[field["name"]] = candidate
        return candidate

    def nested_ref(self, record: dict[str, Any], field: dict[str, Any]) -> str:
        """The expression passed to `field_nested_dyn`."""
        name = self.ident(record, field)
        if field.get("nested") in self.serializers:
            return (
                f"self.{name}"
                ".map(|value| value as &(dyn crate::ser::DrfSerialize + '_))"
            )
        return f"self.{name}"

    def scalar_expr(
        self,
        field: dict[str, Any],
        kind: str,
        many: bool,
        *,
        source: str,
        is_ref: bool,
    ) -> str:
        """The `default` expression for `field_dyn`."""
        deref = f"*{source}" if is_ref else source
        optional = self.optional(field)
        if many and field.get("allow_null") and not self.skippable(field):
            wrapper = {
                "uuid": "DrfUuidList",
                "str": "DrfStrList",
                "int": "DrfIntList",
                "float": "DrfFloatList",
            }[kind]
            return f"&{source}.map(crate::fields::{wrapper})"
        if kind == "uuid" and many:
            return f"&crate::fields::DrfUuidList({source})"
        if kind == "str" and many:
            return f"&crate::fields::DrfStrList({source})"
        if kind == "int" and many:
            return f"&crate::fields::DrfIntList({source})"
        if kind == "float" and many:
            return f"&crate::fields::DrfFloatList({source})"
        if kind == "uuid":
            if optional:
                return f"&{source}.map(crate::fields::DrfUuid)"
            if is_ref:
                return f"&crate::fields::DrfUuid(*{source})"
            return f"&crate::fields::DrfUuid({source})"
        if kind == "datetime":
            if optional:
                return f"&crate::fields::drf_datetime_opt({source}, ctx.tz())"
            return f"&crate::fields::drf_datetime({deref}, ctx.tz())"
        if kind == "date":
            if optional:
                return f"&{source}.map(crate::fields::PyDate)"
            return f"&crate::fields::PyDate({deref})"
        if kind == "time":
            if optional:
                return f"&{source}.map(crate::fields::PyTime)"
            return f"&crate::fields::PyTime({deref})"
        if kind == "decimal":
            decimal = field["decimal"]
            max_digits = decimal["max_digits"]
            decimal_places = decimal["decimal_places"]
            value = f"(*{source}).clone()" if is_ref else f"{source}.clone()"
            if decimal["coerce_to_string"]:
                if optional:
                    return (
                        f"&{source}.clone().map(|value| "
                        f"crate::fields::DrfDecimal::new(value, {max_digits}, {decimal_places}))"
                    )
                return (
                    f"&crate::fields::DrfDecimal::new({value}, "
                    f"{max_digits}, {decimal_places})"
                )
            return f"&crate::fields::EncoderDecimal::Drf({value})"
        return f"&{source}"

    def field_write(self, record: dict[str, Any], field: dict[str, Any]) -> str:
        name = self.ident(record, field)
        key = field["name"]
        kind, many = self.element_kind(field)

        if field.get("nested"):
            if field.get("nested_many"):
                return f'obj.field_nested_list_dyn("{key}", ctx, self.{name})?;'
            return f'obj.field_nested_dyn("{key}", ctx, {self.nested_ref(record, field)})?;'

        if kind in ("method", "value"):
            return f'obj.field_value_dyn("{key}", ctx, &self.{name})?;'
        if kind == "pk":
            return f'obj.field_dyn("{key}", ctx, &self.{name})?;'
        if kind == "json":
            return f'obj.field_json_dyn("{key}", ctx, self.{name})?;'
        if kind == "bytes":
            return (
                f'obj.field_dyn("{key}", ctx, '
                f"&self.{name}.map(crate::fields::PyBase64))?;"
            )

        expression = self.scalar_expr(
            field, kind, many, source=f"self.{name}", is_ref=False
        )
        return f'obj.field_dyn("{key}", ctx, {expression})?;'

    def render_module(self, module: str, records: list[dict[str, Any]]) -> str:
        lines: list[str] = []
        lines.append("// Copyright (c) 2023-present Plane Software, Inc. and contributors")
        lines.append("// SPDX-License-Identifier: AGPL-3.0-only")
        lines.append("// See the LICENSE file for details.")
        lines.append("")
        lines.append(f"//! Generated from `{module}` by `tools/inventory/serializers.py --rust`.")
        lines.append("//!")
        lines.append("//! Do not edit. Re-run the generator after changing `apps/api`.")
        lines.append("")
        lines.append("")
        for record in records:
            lines.extend(self.render_struct(record))
            lines.append("")
        return "\n".join(lines).rstrip() + "\n"

    def render_struct(self, record: dict[str, Any]) -> list[str]:
        name = rust_struct_name(record)
        lifetime = "<'a>" if self.struct_lifetime(record) else ""
        lines: list[str] = []
        model = record.get("model") or "no model"
        lines.append(f"/// `{record['name']}` (model: `{model}`).")
        lines.append("///")
        lines.append("/// Fields are in DRF `_readable_fields` order, which is the")
        lines.append("/// `to_representation` key order.")
        lines.append("#[derive(Clone, Debug)]")
        lines.append(f"pub struct {name}{lifetime} {{")
        for field in record["fields"]:
            rust_type, _ = self.field_type(field)
            source = field["source"]
            lines.append(
                f"    /// `{field['name']}`: {field['class']} (source `{source}`)."
            )
            lines.append(f"    pub {self.ident(record, field)}: {rust_type},")
        lines.append("}")
        lines.append("")
        mode = {
            "dynamic": "OutputOnly",
            "v1": "RemoveFields",
            "simple": "None",
        }[record["codegen"]]
        lines.append(f"impl{lifetime} {name}{lifetime} {{")
        lines.append("    /// `_readable_fields` order.")
        fields = ", ".join(
            json.dumps(field["name"]) for field in record["fields"]
        )
        lines.append(f"    pub const FIELDS: &'static [&'static str] = &[{fields}];")
        lines.append(
            "    /// `fields=` semantics of the base class this serializer inherits."
        )
        lines.append(
            f"    pub const MODE: crate::ser::FilterMode = crate::ser::FilterMode::{mode};"
        )
        lines.append("}")
        lines.append("")
        lines.append(
            f"impl{lifetime} crate::ser::DrfSerialize for {name}{lifetime} {{"
        )
        lines.append("    fn drf_write(")
        lines.append("        &self,")
        lines.append("        w: &mut crate::writer::DrfWriter,")
        lines.append("        ctx: &crate::ser::RenderCtx<'_>,")
        lines.append("    ) -> Result<(), crate::DrfError> {")
        lines.append("        let mut obj = w.begin_object();")
        for field in record["fields"]:
            key = json.dumps(field["name"])
            lines.append(f"        if ctx.emits({key}, Self::MODE) {{")
            if self.skippable(field):
                name = self.ident(record, field)
                lines.append(
                    f"            obj.field_optional_dyn({key}, ctx, &self.{name})?;"
                )
            else:
                lines.append(f"            {self.field_write(record, field)}")
            lines.append("        }")
        lines.append("        ctx.expansions()")
        lines.append(
            "            .write_injected(&mut obj, ctx, Self::FIELDS, Self::MODE)?;"
        )
        lines.append("        obj.end();")
        lines.append("        Ok(())")
        lines.append("    }")
        lines.append("}")
        return lines


# ---------------------------------------------------------------------------
# Markdown
# ---------------------------------------------------------------------------


def render_markdown(payload: OrderedDict[str, Any]) -> str:
    serializers = payload["serializers"]
    views = payload["views"]
    counts: dict[str, int] = {}
    for record in serializers:
        counts[record["codegen"]] = counts.get(record["codegen"], 0) + 1
    total = len(serializers)
    fields = sum(record["field_count"] for record in serializers)

    lines: list[str] = []
    lines.append("# Serializer inventory")
    lines.append("")
    lines.append(
        "Generated from DRF's own introspection by "
        "`apps/server/tools/inventory/serializers.py`. Do not edit by hand: "
        "re-run the generator. The machine-readable form is `serializers.json`; "
        "this file is the reviewable view of it."
    )
    lines.append("")
    lines.append("## How this was produced")
    lines.append("")
    lines.append(
        "Every serializer module in `plane.app.serializers`, "
        "`plane.api.serializers`, `plane.space.serializer` and "
        "`plane.license.api.serializers` is imported, and every distinct "
        "`BaseSerializer` subclass is instantiated with no instance and no "
        "context (the way `XSerializer(qs, many=True)` does). Fields are read "
        "from `_readable_fields`, which is the order `to_representation` "
        "writes keys in, and each field's properties are recorded together "
        "with the Django model field its `source` resolves to."
    )
    lines.append("")
    lines.append(
        "`fields=` and `expand=` are recorded per serializer under "
        "`expansion`. `simple` / `dynamic` / `v1` serializers are code-generated "
        "into `crates/plane-serializers/src/generated/`; `manual` serializers "
        "have a hand-written `to_representation` (or a `get_fields`/`__init__` "
        "override, or an underivable field type) and need a hand-written Rust "
        "module, which is the next session's work. Nothing here is stubbed: a "
        "manual serializer has no generated Rust type at all."
    )
    lines.append("")
    lines.append("## Counts")
    lines.append("")
    lines.append("| Kind | Serializers |")
    lines.append("| --- | ---: |")
    for kind in ("simple", "dynamic", "v1", "manual"):
        lines.append(f"| {kind} | {counts.get(kind, 0)} |")
    lines.append(f"| **total** | **{total}** |")
    lines.append("")
    lines.append(
        f"{fields} readable fields, "
        f"{sum(len(record['write_only_fields']) for record in serializers)} "
        f"write-only fields recorded for Phase 5, "
        f"{len(views)} views mapped."
    )
    lines.append("")

    lines.append("## Rust runtime")
    lines.append("")
    lines.append(
        "`crates/plane-serializers` renders a serializer object with the same "
        "shape and bytes as `JSONRenderer().render(serializer.data)`. A "
        "generated struct is plain data (`Clone, Debug`) in output order; "
        "`drf_write` checks the field mask, consults the resolved expansions "
        "and writes each field through the wrapper for its DRF class:"
    )
    lines.append("")
    lines.append("| DRF field class | Rust wrapper | Python |")
    lines.append("| --- | --- | --- |")
    lines.append("| `UUIDField`, UUID `PrimaryKeyRelatedField` | `DrfUuid` | `str(uuid)` |")
    lines.append(
        "| integer `PrimaryKeyRelatedField` | `i64` | JSON number |"
    )
    lines.append(
        "| `PrimaryKeyRelatedField` with no resolvable model (annotation) | "
        "`DrfPk` | `str(uuid)` / number / string / `null` |"
    )
    lines.append(
        "| `DateTimeField` | `DrfDateTime` in the request's active timezone | "
        "`isoformat()`, `+00:00` → `Z` |"
    )
    lines.append("| `DateField` / `TimeField` | `PyDate` / `PyTime` | `isoformat()` |")
    lines.append(
        "| `DecimalField` | `DrfDecimal` (`coerce_to_string=True`) or "
        "`EncoderDecimal::Drf` | quantised string / `float(value)` |"
    )
    lines.append(
        "| `JSONField` | `PgJson` from `plane_db::JsonText` | "
        "`transcode_json` keeps Postgres key order |"
    )
    lines.append(
        "| `ModelField(BinaryField)` | `PyBase64` | "
        "`b64encode(value).decode(\"ascii\")` |"
    )
    lines.append("| `ListField` of UUID/str/int/float | `DrfUuidList` / `DrfStrList` / `DrfIntList` / `DrfFloatList` | a JSON array |")
    lines.append(
        "| `SerializerMethodField`, `ReadOnlyField` over an annotation | "
        "`DrfValue` | whatever the handler supplies |"
    )
    lines.append("| nested serializer | `&Nested`, `Option<&Nested>`, `&[Nested]` | an object / `null` / array |")
    lines.append("")
    lines.append(
        "`fields=` is `FieldEntry` (`Name` or the dict-item `Nested`; only the "
        "top-level key matters, as in `_filter_fields`). `expand=` is "
        "`Expansions`: the handler resolves every requested name to a "
        "`DrfValue` and `write_injected` appends the injectable ones after the "
        "base fields. A requested mapper name with no value is an error rather "
        "than a silently unexpanded field, matching Django's `AttributeError`."
    )
    lines.append("")
    lines.append(
        "`SkipField` is modelled as `FieldValue<T>` on annotation-backed "
        "fields with `required=False`: `Absent` omits the key, `Null` writes "
        "`null` (the attribute exists and is `None`, which DRF writes as "
        "`null` without calling the field) and `Value(v)` renders the value. A "
        "nullable model column stays `Option<T>` because its attribute always "
        "exists; the two are never mixed because `allow_null` fields are not "
        "skippable. `None` for a *value* is written as `null` by "
        "`DrfWriter::write_value`, which compensates for "
        "`JSONRenderer.render(None) == b\"\"` only applying at the top level."
    )
    lines.append("")
    lines.append("## Golden corpus")
    lines.append("")
    lines.append(
        "`crates/plane-serializers/tests/golden/serializers.jsonl` holds 110 "
        "renders recorded by `tools/golden/serializers_record.py` from a real "
        "Django process (`tools/golden/serializers_fixture.py` builds the rows "
        "in a `t_*` clone). `crates/plane-serializers/tests/golden.rs` builds "
        "the same values and asserts byte equality. Covered: no arguments in "
        "`UTC`, `Asia/Kolkata` and `America/New_York` (2026-11-01, the US DST "
        "fall-back), `fields=` lists and the dict form, every `expand=` shape, "
        "`fields=` and `expand=` together, `many=True`, a skippable annotation "
        "that is present and `None` (written as `null`) next to one that is "
        "missing (`SkipField`), and a probe serializer "
        "for decimal/time/binary/JSON formatting Plane's models do not exercise."
    )
    lines.append("")
    lines.append("## Manual serializers")
    lines.append("")
    lines.append(
        "These need a hand-written Rust module (Phase 5 for most of them):"
    )
    lines.append("")
    for record in serializers:
        if record["codegen"] == "manual":
            lines.append(
                f"* `{record['name']}` — {record.get('manual_reason', '')}"
            )
    lines.append("")
    lines.append("## Expansion mapper")
    lines.append("")
    lines.append("`get_expansion_mapper()` returns:")
    lines.append("")
    lines.append("| `expand` name | Serializer | Many |")
    lines.append("| --- | --- | --- |")
    many = payload["expansion_mapper"]["many"]
    for name, serializer in payload["expansion_mapper"]["mapping"].items():
        lines.append(
            f"| `{name}` | `{serializer.rsplit('.', 1)[-1]}` | "
            f"{'yes' if name in many else 'no'} |"
        )
    lines.append("")
    lines.append(
        "`issue_attachments` is not in the mapper and is always appended last "
        "when requested (`plane/app/serializers/base.py:171-188`)."
    )
    lines.append("")

    if payload["choice_sets"]:
        lines.append("## Choice sets")
        lines.append("")
        lines.append(
            "`ChoiceField.choices` lists longer than "
            f"{CHOICES_INLINE_LIMIT} entries are stored once here and referenced "
            "from the field as `choices_ref`, instead of being repeated in every "
            "serializer that uses them. The field table still names the set and "
            "its size."
        )
        lines.append("")
        lines.append("| Set | Entries |")
        lines.append("| --- | ---: |")
        for key, choices in payload["choice_sets"].items():
            lines.append(f"| `{key}` | {len(choices)} |")
        lines.append("")

    lines.append("## Serializer index")
    lines.append("")
    lines.append("| Serializer | Model | Kind | Fields | Overrides |")
    lines.append("| --- | --- | --- | ---: | --- |")
    for record in serializers:
        overrides = []
        for key in ("to_representation", "get_fields", "__init__", "create", "update"):
            owner = record["overrides"].get(key)
            if owner and owner.rsplit(".", 1)[-1] not in (
                "Serializer",
                "ModelSerializer",
                "BaseSerializer",
                "DynamicBaseSerializer",
            ):
                overrides.append(f"{key}@{owner.rsplit('.', 1)[-1]}")
        if record["overrides"].get("validate"):
            overrides.append("validate: " + ", ".join(record["overrides"]["validate"]))
        short = record["name"].split("plane.", 1)[-1]
        lines.append(
            f"| `{record['class']}` ({short}) | `{record['model'] or ''}` | "
            f"{record['codegen']} | {record['field_count']} | "
            f"{'; '.join(overrides) or ''} |"
        )
    lines.append("")

    lines.append("## Serializers")
    lines.append("")
    for record in serializers:
        lines.append(f"### `{record['name']}`")
        lines.append("")
        lines.append(
            f"Model: `{record['model']}` · base: `{record['base']}` · "
            f"codegen: **{record['codegen']}**"
        )
        if record.get("manual_reason"):
            lines.append("")
            lines.append(f"Manual because: {record['manual_reason']}.")
        if record["abstract"]:
            lines.append("")
            lines.append("Abstract base class: no `Meta.model`, not instantiable.")
        lines.append("")
        if record["fields"]:
            lines.append("| # | Field | DRF class | Source | RO | WO | Null | Type | Model field | Notes |")
            lines.append("| ---: | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
            for index, field in enumerate(record["fields"], start=1):
                notes = []
                if field.get("nested"):
                    notes.append(
                        ("many " if field.get("nested_many") else "")
                        + "nested " + field["nested"].rsplit(".", 1)[-1]
                    )
                if field.get("method_name"):
                    notes.append(f"method {field['method_name']}")
                if field.get("format"):
                    notes.append(
                        "format " + str(field["format"])
                        if field["format"] != "iso-8601"
                        else "ISO 8601"
                    )
                if field.get("choices"):
                    notes.append(f"{len(field['choices'])} choices")
                if field.get("choices_ref"):
                    notes.append(
                        f"{field['choices_count']} choices "
                        f"(see choice set `{field['choices_ref']}`)"
                    )
                if field.get("pk"):
                    notes.append(f"pk {field['pk'].get('python_type')}")
                if field.get("json"):
                    notes.append("jsonb/text")
                if field.get("child"):
                    notes.append(f"child {field['child']['class']}")
                if field.get("decimal"):
                    notes.append(
                        f"decimal {field['decimal']['max_digits']},"
                        f"{field['decimal']['decimal_places']}"
                    )
                model_field = field.get("model_field")
                model_text = ""
                if model_field:
                    model_text = f"{model_field['class']}"
                    if field.get("attribute") == "property":
                        model_text += " (property)"
                elif field.get("attribute") in ("property", "annotation"):
                    model_text = field["attribute"]
                lines.append(
                    "| {index} | `{name}` | {cls} | `{source}` | {ro} | {wo} | "
                    "{null} | {pytype} | {model_field} | {notes} |".format(
                        index=index,
                        name=field["name"],
                        cls=field["class"],
                        source=field["source"],
                        ro="yes" if field["read_only"] else "",
                        wo="yes" if field["write_only"] else "",
                        null="yes" if field["allow_null"] else "",
                        pytype=field.get("python_type") or "",
                        model_field=model_text,
                        notes="; ".join(notes),
                    )
                )
            lines.append("")
        if record["write_only_fields"]:
            lines.append("Write-only fields (not in the response; Phase 5):")
            lines.append("")
            lines.append("| Field | DRF class | Source | Child |")
            lines.append("| --- | --- | --- | --- |")
            for field in record["write_only_fields"]:
                child = field.get("child")
                lines.append(
                    f"| `{field['name']}` | {field['class']} | `{field['source']}` | "
                    f"{child['class'] if child else ''} |"
                )
            lines.append("")

    lines.append("## View → serializer index")
    lines.append("")
    lines.append(
        "Every unique view in `routes.json`. `serializer_class` is the class "
        "attribute; `get_serializer_class` shows the method's file:line and the "
        "serializer names it can return; `calls` lists explicit `XSerializer(` "
        "calls in the view's own methods (and one level of local helper "
        "functions), with `fields=`/`expand=`/`many=` arguments."
    )
    lines.append("")
    lines.append("| View | Routes | `serializer_class` | `get_serializer_class` | Explicit calls |")
    lines.append("| --- | ---: | --- | --- | --- |")
    for view in views:
        serializer_class = view.get("serializer_class") or ""
        if serializer_class:
            serializer_class = f"`{serializer_class.rsplit('.', 1)[-1]}`"
        getter = view.get("get_serializer_class")
        getter_text = ""
        if getter:
            returns = ", ".join(f"`{name}`" for name in getter.get("returns", []))
            getter_text = f"{getter.get('file', '')}:{getter.get('line', '')} → {returns}"
        calls = view.get("calls", [])
        call_text = "<br>".join(
            "`{name}` ({file}:{line}, {method}{kwargs})".format(
                name=call["serializer"],
                file=(call.get("file") or "").rsplit("/", 1)[-1],
                line=call["line"],
                method=call["method"],
                kwargs=(
                    ""
                    if not call["kwargs"]
                    else " " + ", ".join(f"{key}={value}" for key, value in call["kwargs"].items())
                ),
            )
            for call in calls
        )
        lines.append(
            f"| `{view['view']}` | {len(view.get('paths', []))} | "
            f"{serializer_class} | {getter_text} | {call_text} |"
        )
    lines.append("")
    return "\n".join(lines).rstrip() + "\n"


# ---------------------------------------------------------------------------
# Write field tables (plane-http-app/src/write/generated)
# ---------------------------------------------------------------------------
#
# A write handler validates a request by calling `Serializer`'s field helpers
# in DRF's order. The order and the per-field options are Python metadata, not
# something a human should retype per serializer, so this section emits them:
#
# * `FIELDS` -- writable fields in `self.fields` order (DRF's validation order)
#   with the kind and the effective options (after `extra_kwargs`);
# * `READ_ONLY_FIELDS` -- declared read-only fields, `Meta.read_only_fields`,
#   and `editable=False` model fields (including ones DRF drops entirely);
# * `EXTRA_KWARGS` -- the `Meta.extra_kwargs` overrides, verbatim;
# * `validate` -- the helper calls a handler would make today, with
#   `// TODO(kind)` for a field class `plane-http-app/src/write` has no helper
#   for. It deliberately does not invent helpers.
#
# The output is one module per serializer plus a `mod.rs` that defines the
# table types. It lands in `crates/plane-http-app/src/write/generated/`; the
# representation modules in `plane-serializers` are separate and unchanged.

WRITE_GENERATED_DIR = (
    SERVER_DIR / "crates" / "plane-http-app" / "src" / "write" / "generated"
)

# The work-items domain (routes in `apps/api/plane/app/views/issue/` and the
# draft viewset), plus the two pilots already hand-ported. The exact set is
# reviewed when a domain starts; `--write-serializers` overrides it.
WRITE_SERIALIZERS = (
    "plane.app.serializers.issue.IssueCreateSerializer",
    "plane.app.serializers.issue.IssueSerializer",
    "plane.app.serializers.issue.IssueCommentSerializer",
    "plane.app.serializers.issue.CommentReactionSerializer",
    "plane.app.serializers.issue.IssueLinkSerializer",
    "plane.app.serializers.issue.IssueAttachmentSerializer",
    "plane.app.serializers.issue.IssueReactionSerializer",
    "plane.app.serializers.issue.IssueRelationSerializer",
    "plane.app.serializers.issue.IssueSubscriberSerializer",
    "plane.app.serializers.draft.DraftIssueCreateSerializer",
    "plane.app.serializers.draft.DraftIssueSerializer",
    "plane.app.serializers.draft.DraftIssueDetailSerializer",
    "plane.app.serializers.workspace.StickySerializer",
    "plane.app.serializers.project.ProjectSerializer",
    "plane.app.serializers.state.StateSerializer",
    "plane.app.serializers.project.ProjectMemberSerializer",
    "plane.app.serializers.project.DeployBoardSerializer",
    "plane.app.serializers.issue.ProjectUserPropertySerializer",
    "plane.app.serializers.issue.LabelSerializer",
    "plane.app.serializers.cycle.CycleWriteSerializer",
    "plane.app.serializers.cycle.CycleUserPropertiesSerializer",
    "plane.app.serializers.module.ModuleWriteSerializer",
    "plane.app.serializers.module.ModuleUserPropertiesSerializer",
    "plane.app.serializers.module.ModuleLinkSerializer",
    "plane.app.serializers.module.ModuleIssueSerializer",
    "plane.app.serializers.view.IssueViewSerializer",
    "plane.app.serializers.page.PageBinaryUpdateSerializer",
    "plane.app.serializers.page.PageSerializer",
    "plane.app.serializers.page.PageDetailSerializer",
    "plane.app.serializers.intake.IntakeSerializer",
    "plane.app.serializers.intake.IntakeIssueSerializer",
    "plane.app.serializers.notification.UserNotificationPreferenceSerializer",
    "plane.app.serializers.webhook.WebhookSerializer",
    "plane.app.serializers.favorite.UserFavoriteSerializer",
    "plane.app.serializers.estimate.EstimatePointSerializer",
    "plane.app.serializers.analytic.AnalyticViewSerializer",
    # users/me writes: `UserEndpoint.partial_update`, `ProfileEndpoint.patch`
    # and `ApiTokenEndpoint.post`/`patch` (`plans/USERS_ME.md` §2f).
    "plane.app.serializers.user.UserSerializer",
    "plane.app.serializers.user.ProfileSerializer",
    "plane.app.serializers.api.APITokenSerializer",
    # workspace-core writes (`plans/WORKSPACES_CORE.md` §2c): the seven
    # serializers routes 186/187/188/189/194/199/200/207/218/219/220/221 use.
    # `WorkSpaceMemberSerializer` is included for its one writable field
    # (`role`); every other field is read-only, which the table records.
    "plane.app.serializers.workspace.WorkSpaceSerializer",
    "plane.app.serializers.workspace.WorkspaceThemeSerializer",
    "plane.app.serializers.workspace.WorkSpaceMemberInviteSerializer",
    "plane.app.serializers.workspace.WorkspaceUserPropertiesSerializer",
    "plane.app.serializers.workspace.WorkspaceUserLinkSerializer",
    "plane.app.serializers.workspace.WorkspaceHomePreferenceSerializer",
    "plane.app.serializers.workspace.WorkSpaceMemberSerializer",
)

# Hand-written tables the generated output must agree with. Each value is
# `(Rust file, function containing the field calls)`. This is only used by
# `--write-equivalence`; the generated modules themselves never depend on it.
WRITE_EQUIVALENCE = {
    "plane.app.serializers.workspace.StickySerializer": (
        SERVER_DIR / "crates/plane-http-app/src/stickies/write.rs",
        "fn apply_body",
    ),
    "plane.app.serializers.project.ProjectSerializer": (
        SERVER_DIR / "crates/plane-http-app/src/projects/write.rs",
        "fn read_body",
    ),
    "plane.app.serializers.state.StateSerializer": (
        SERVER_DIR / "crates/plane-http-app/src/states/write.rs",
        "fn read_body",
    ),
    "plane.app.serializers.project.ProjectMemberSerializer": (
        SERVER_DIR / "crates/plane-http-app/src/projects/member_write.rs",
        "fn apply_body",
    ),
    "plane.app.serializers.project.DeployBoardSerializer": (
        SERVER_DIR / "crates/plane-http-app/src/projects/deploy_board_write.rs",
        "pub fn partial_update",
    ),
    "plane.app.serializers.issue.ProjectUserPropertySerializer": (
        SERVER_DIR / "crates/plane-http-app/src/projects/settings_write.rs",
        "pub fn user_properties",
    ),
}

FIELD_KIND_VARIANTS = {
    "char": "Char",
    # `serializers.SlugField`: a `CharField` whose validator chain is the model
    # field's own validators, DRF's `UniqueValidator`, `MaxLength`, the null and
    # surrogate checks, and `RegexValidator(r"^[-a-zA-Z0-9_]+$")`
    # (`rest_framework/fields.py:790-803`). Its own kind so the table shows it
    # and `validate` points the handler at `Serializer::slug_field`, which runs
    # the chain in that order (`Workspace.slug`, `plane/app/serializers/
    # workspace.py:43-86`).
    "slug": "Slug",
    # `serializers.URLField`: a `CharField` with Django's `URLValidator` and
    # optionally declared validators appended (`rest_framework/fields.py:806-814`).
    # Its own kind so `validate` can emit `url_field` instead of `char_field`.
    "url": "Url",
    "integer": "Integer",
    "float": "Float",
    "boolean": "Boolean",
    "choice": "Choice",
    "multiple_choice": "Choice",
    "datetime": "DateTime",
    "date": "Date",
    "time": "Time",
    "uuid": "Uuid",
    "primary_key": "PrimaryKey",
    "json": "Json",
    "list": "List",
    "many_related": "ManyRelated",
    "nested": "Nested",
    "nested_many": "List",
    "serializer_method": "SerializerMethod",
    "model_field": "ModelField",
    "file": "Other",
    "decimal": "Other",
    "other": "Other",
}


def write_kind(field: Any) -> str:
    """The field class behind a DRF field instance, for the table and validate."""
    from rest_framework import serializers as drf

    if isinstance(field, drf.SerializerMethodField):
        return "serializer_method"
    if isinstance(field, drf.ListSerializer):
        return "nested_many"
    if isinstance(field, drf.BaseSerializer):
        return "nested"
    if isinstance(field, drf.ManyRelatedField):
        return "many_related"
    if isinstance(field, drf.ListField):
        return "list"
    if isinstance(field, drf.PrimaryKeyRelatedField):
        return "primary_key"
    if isinstance(field, drf.MultipleChoiceField):
        return "multiple_choice"
    if isinstance(field, drf.ChoiceField):
        return "choice"
    if isinstance(field, drf.BooleanField):
        return "boolean"
    if isinstance(field, drf.UUIDField):
        return "uuid"
    if isinstance(field, drf.IntegerField):
        return "integer"
    if isinstance(field, drf.FloatField):
        return "float"
    if isinstance(field, drf.DecimalField):
        return "decimal"
    if isinstance(field, drf.DateTimeField):
        return "datetime"
    if isinstance(field, drf.DateField):
        return "date"
    if isinstance(field, drf.TimeField):
        return "time"
    if isinstance(field, drf.JSONField):
        return "json"
    from rest_framework import fields as drf_fields

    if isinstance(field, drf_fields.FileField):
        return "file"
    # Before `CharField`: `URLField` subclasses it
    # (`rest_framework/fields.py:794-814`).
    if isinstance(field, drf.URLField):
        return "url"
    # Before `CharField`: `SlugField` subclasses it (`fields.py:790-803`).
    if isinstance(field, drf.SlugField):
        return "slug"
    if isinstance(field, drf.CharField):
        return "char"
    if isinstance(field, drf.ModelField):
        return "model_field"
    return "other"


def python_repr(value: Any) -> str:
    """A deterministic Python-ish repr for a table value (never evaluated)."""
    if value is None:
        return "None"
    if isinstance(value, bool):
        return "True" if value else "False"
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return repr(value)
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(python_repr(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ", ".join(f"{python_repr(key)}: {python_repr(item)}" for key, item in value.items()) + "}"
    return repr(value)


def rust_string(value: str) -> str:
    escaped = (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )
    return f'"{escaped}"'


def bool_literal(value: bool) -> str:
    return "true" if value else "false"


def write_field_record(field: Any, cls: type, many_related: bool = False) -> OrderedDict[str, Any]:
    """One entry of a serializer's write table.

    ``many_related`` is set for a ``ListField``'s child, whose pk lookup is
    compiled as ``pk__in`` with one element (the child itself runs one
    ``get(pk=…)`` per element).
    """
    from rest_framework import fields as drf_fields
    from rest_framework import relations as drf_relations
    from rest_framework import serializers as drf_serializers

    record: OrderedDict[str, Any] = OrderedDict()
    # A `ListField`'s child is unbound: it has no name of its own.
    record["name"] = field.field_name or "element"
    record["python_class"] = type(field).__name__
    record["kind"] = write_kind(field)
    record["source"] = field.source or record["name"]
    record["required"] = bool(field.required)
    record["read_only"] = bool(field.read_only)
    record["write_only"] = bool(field.write_only)
    record["allow_null"] = bool(field.allow_null)
    allow_blank = getattr(field, "allow_blank", None)
    record["allow_blank"] = bool(allow_blank) if allow_blank is not None else False
    record["allow_empty"] = bool(getattr(field, "allow_empty", True))
    record["trim_whitespace"] = bool(getattr(field, "trim_whitespace", True))
    if field.default is not drf_fields.empty:
        record["default"] = python_repr(field.default)

    for key, attribute, cast in (
        ("max_length", "max_length", int),
        ("min_length", "min_length", int),
        ("min_value", "min_value", int),
        ("max_value", "max_value", int),
    ):
        value = getattr(field, attribute, None)
        if value is not None:
            try:
                record[key] = cast(value)
            except (TypeError, ValueError):
                record[key] = str(value)

    if isinstance(field, drf_fields.ChoiceField):
        # Only `ChoiceField` is safe to iterate: `RelatedField.choices` runs a
        # queryset (see the module docstring).
        record["choices"] = [(python_repr(value), str(display)) for value, display in field.choices.items()]
        record["choice_values"] = [value for value, _ in field.choices.items()]

    if isinstance(field, (drf_fields.DateTimeField, drf_fields.DateField, drf_fields.TimeField)):
        from rest_framework.settings import api_settings

        # `input_formats` is only an instance attribute when the declaration
        # passed it; otherwise DRF reads the setting at parse time.
        if isinstance(field, drf_fields.DateTimeField):
            default_formats = api_settings.DATETIME_INPUT_FORMATS
        elif isinstance(field, drf_fields.DateField):
            default_formats = api_settings.DATE_INPUT_FORMATS
        else:
            default_formats = api_settings.TIME_INPUT_FORMATS
        formats = getattr(field, "input_formats", None) or default_formats
        record["input_formats"] = [str(item) for item in formats]

    if isinstance(field, (drf_relations.PrimaryKeyRelatedField, drf_relations.ManyRelatedField)):
        queryset = related_queryset(field)
        model = getattr(queryset, "model", None)
        record["related_model"] = model_label(model)
        record["related_table"] = model._meta.db_table if model is not None else None
        # The exact lookup DRF runs for this field. It is not just the table's
        # `deleted_at` column: the field's own queryset decides.
        # `IssueCreateSerializer.state_id` declares `State.all_state_objects`
        # (a plain manager) and accepts a soft-deleted or triage state, while an
        # auto-generated FK reads `State.objects` (`StateManager`) and rejects
        # both.
        if queryset is not None:
            record["related_query"] = pk_lookup_query(
                field, many_related or isinstance(field, drf_relations.ManyRelatedField)
            )

    if isinstance(field, drf_fields.ListField):
        # A `ListField` of `PrimaryKeyRelatedField` children validates each
        # element with the child's `queryset.get(pk=…)`; the child record is
        # compiled with `pk__in` so its statement serves one element per query.
        record["child"] = write_field_record(field.child, cls, many_related=True)
        if record["child"].get("related_query") is not None:
            record["related_query"] = record["child"]["related_query"]
    elif isinstance(field, drf_relations.ManyRelatedField):
        record["child"] = write_field_record(field.child_relation, cls)
    elif isinstance(field, drf_serializers.ListSerializer):
        record["child"] = write_field_record(field.child, cls)
    if record["kind"] == "url":
        # The validators DRF appends itself (`CharField.__init__`'s
        # `ProhibitNullCharactersValidator`/`ProhibitSurrogateCharactersValidator`
        # and `URLField.__init__`'s `URLValidator`) are not part of the
        # declaration: `url_field` runs them in the same positions. Only the
        # serializer's own list is named here, and an unknown one is a TODO
        # rather than a silent drop.
        record["url_validators"] = declared_url_validators(field)
    return record


# `WebhookSerializer.url` is the app's only `serializers.URLField`, and its two
# declared validators are `plane.db.models.webhook`'s
# (`plane/app/serializers/webhook.py:25`).
URL_FIELD_VALIDATORS = {
    "validate_schema": "Schema",
    "validate_domain": "Domain",
}


def declared_url_validators(field: Any) -> list[str] | None:
    """The `UrlValidator` variants for a `URLField`, or `None` when unknown."""
    from django.core.validators import (
        MaxLengthValidator,
        MinLengthValidator,
        ProhibitNullCharactersValidator,
        URLValidator,
    )
    from rest_framework.validators import ProhibitSurrogateCharactersValidator

    # `CharField.__init__` appends the length validators it derives from
    # `max_length`/`min_length` and the two character-class validators;
    # `URLField.__init__` appends `URLValidator`
    # (`rest_framework/fields.py:537-559`, `:806-814`). `url_field` runs each of
    # them in the same position, so they must not appear as declarations.
    builtin = (
        MaxLengthValidator,
        MinLengthValidator,
        ProhibitNullCharactersValidator,
        ProhibitSurrogateCharactersValidator,
        URLValidator,
    )
    names: list[str] = []
    for validator in field.validators:
        if isinstance(validator, builtin):
            continue
        mapped = URL_FIELD_VALIDATORS.get(getattr(validator, "__name__", ""))
        if mapped is None:
            return None
        names.append(mapped)
    return names


def write_module_name(class_name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", class_name).lower()


def related_queryset(field: Any) -> Any:
    """The queryset a related field validates against.

    DRF's `PrimaryKeyRelatedField.get_queryset` returns `self.queryset`, and a
    `ManyRelatedField` delegates to its `child_relation`; either can be a model
    **manager** rather than a `QuerySet`.
    """
    from rest_framework import relations as drf_relations

    if isinstance(field, drf_relations.ManyRelatedField):
        queryset = getattr(field.child_relation, "queryset", None)
    else:
        queryset = getattr(field, "queryset", None)
    if queryset is not None and not hasattr(queryset, "query"):
        queryset = queryset.all()
    return queryset


def queryset_filters_deleted_at(queryset: Any, table: str) -> bool:
    """Whether this queryset's compiled SQL filters `"<table>"."deleted_at" IS NULL`.

    The predecessor of [`pk_lookup_query`], kept as the independent oracle the
    generator's self-check compares against: compiling the whole queryset must
    agree with this predicate's `deleted_at` half for every related field.
    """
    if queryset is None:
        return False
    try:
        sql = str(queryset.query)
    except Exception:
        return False
    return f'"{table}"."deleted_at" IS NULL' in sql


# A fixed pk used to compile each related field's lookup. Its value never
# appears in the emitted SQL: Django binds it as a `%s` parameter and
# `pk_lookup_query` rewrites that one placeholder to `$1`.
PK_LOOKUP_SENTINEL = uuid.UUID("00000000-0000-0000-0000-000000000000")
# Stands in for the pk parameter while the queryset's own parameters are
# rendered as literals. Must not occur in the compiled SQL.
PK_LOOKUP_MARK = "<plane-pk>"


def sql_parameter_literal(value: Any) -> str:
    """One compiled queryset parameter as a Postgres literal.

    `sql_with_params` returns Django's own constants as separate parameters
    (`'triage'` for the managers' triage exclusion). Only the pk is a real bind
    parameter in the emitted statement, so the rest are inlined here. The
    write serializers' querysets carry strings only today; an unsupported type
    stops generation rather than emitting SQL that might not mean what Django
    compiled.
    """
    import datetime as dt
    import decimal

    if isinstance(value, bool):
        return "TRUE" if value else "FALSE"
    if isinstance(value, decimal.Decimal):
        return str(value)
    if isinstance(value, (int, float)):
        return repr(value)
    if isinstance(value, str):
        return "'" + value.replace("'", "''") + "'"
    if value is None:
        return "NULL"
    if isinstance(value, uuid.UUID):
        # Compared with a `uuid` column, an untyped literal is cast by Postgres.
        return f"'{value}'"
    if isinstance(value, dt.datetime):
        return f"'{value.isoformat()}'"
    if isinstance(value, (dt.date, dt.time)):
        return f"'{value.isoformat()}'"
    raise SystemExit(
        f"error: cannot inline a compiled {type(value).__name__} parameter ({value!r})"
    )


def related_queryset_source(field: Any) -> str:
    """The field's declared queryset class, for the statement's doc."""
    from rest_framework import relations as drf_relations

    target = field.child_relation if isinstance(field, drf_relations.ManyRelatedField) else field
    declared = getattr(target, "queryset", None)
    if declared is None:
        return "unknown"
    return type(declared).__name__


def pk_lookup_query(field: Any, many: bool) -> OrderedDict[str, Any]:
    """Compile DRF's `PrimaryKeyRelatedField.to_internal_value` lookup.

    `rest_framework/relations.py:252-265` does `queryset.get(pk=data)`, and
    `QuerySet.get` is `filter(pk=data)[:21]`. The limit is only there to detect
    `MultipleObjectsReturned`, which a pk lookup cannot hit, so the emitted
    statement drops it and selects only the pk. The WHERE clause — including
    every join the field's manager adds — is exactly what decides whether DRF
    accepts the value.

    A many-related (or `ListField` child) lookup compiles `pk__in` with one
    element: Django renders that as `IN ($1)`, so the same statement serves
    every element, one round trip each, exactly like the child's own
    `get(pk=…)`.
    """
    queryset = related_queryset(field)
    lookup = queryset.filter(pk__in=[PK_LOOKUP_SENTINEL]) if many else queryset.filter(pk=PK_LOOKUP_SENTINEL)
    compiled = lookup.values_list("pk", flat=True).order_by()
    sql, params = compiled.query.sql_with_params()
    params = list(params)
    try:
        pk_index = params.index(PK_LOOKUP_SENTINEL)
    except ValueError:  # pragma: no cover - every lookup filters on the pk
        raise SystemExit(
            f"error: the compiled lookup for {field.field_name!r} has no pk parameter: {sql}"
        )
    literals = [
        PK_LOOKUP_MARK if index == pk_index else sql_parameter_literal(value)
        for index, value in enumerate(params)
    ]
    # `%` formatting also unescapes Django's `%%`, which `sql_with_params`
    # leaves in place for a `LIKE` pattern.
    rendered = sql % tuple(literals)
    if rendered.count(PK_LOOKUP_MARK) != 1:  # pragma: no cover - guarded above
        raise SystemExit(
            f"error: the compiled lookup for {field.field_name!r} binds the pk "
            f"more than once: {rendered}"
        )
    rendered = rendered.replace(PK_LOOKUP_MARK, "$1")
    # Cross-check against the older, predicate-name-based rule: if the queryset
    # filtered `deleted_at`, the compiled statement must too.
    table = queryset.model._meta.db_table
    if queryset_filters_deleted_at(queryset, table) and f'"{table}"."deleted_at" IS NULL' not in rendered:
        raise SystemExit(
            f"error: {field.field_name!r} filters deleted_at but the compiled "
            f"lookup does not: {rendered}"
        )
    return OrderedDict(
        [
            ("sql", rendered),
            ("many", many),
            ("manager", related_queryset_source(field)),
        ]
    )


def write_serializer_record(cls: type) -> OrderedDict[str, Any]:
    """Everything the module for one serializer needs."""
    serializer = cls()
    model = getattr(getattr(cls, "Meta", None), "model", None)
    meta = getattr(cls, "Meta", None)
    meta_read_only = list(getattr(meta, "read_only_fields", None) or [])
    fields = list(serializer.fields.values())
    writable = [write_field_record(field, cls) for field in fields if not field.read_only]
    read_only: list[OrderedDict[str, Any]] = []
    present = {field.field_name for field in fields}
    model_fields = {model_field.name: model_field for model_field in model._meta.get_fields() if getattr(model_field, "concrete", False)} if model is not None else {}

    for field in fields:
        if not field.read_only:
            continue
        if field.field_name in meta_read_only:
            reason = "Meta.read_only_fields"
        elif field.field_name in model_fields and not model_fields[field.field_name].editable:
            reason = "editable=False model field"
        elif isinstance(field, __import__("rest_framework.serializers", fromlist=["SerializerMethodField"]).SerializerMethodField):
            reason = "SerializerMethodField"
        else:
            reason = "read_only"
        read_only.append(
            OrderedDict(
                [
                    ("name", field.field_name),
                    ("python_class", type(field).__name__),
                    ("kind", write_kind(field)),
                    ("source", field.source),
                    ("reason", reason),
                ]
            )
        )

    # `fields=` can omit an `editable=False` model field entirely; DRF drops it
    # from both input and output, but a port still needs to know it is not part
    # of the body contract.
    for name, model_field in sorted(model_fields.items()):
        if name in present or model_field.editable:
            continue
        read_only.append(
            OrderedDict(
                [
                    ("name", name),
                    ("python_class", type(model_field).__name__),
                    ("kind", "model_field"),
                    ("source", name),
                    ("reason", "editable=False model field (dropped by DRF)"),
                ]
            )
        )

    extra_kwargs: list[tuple[str, str, str]] = []
    for name, options in sorted((getattr(meta, "extra_kwargs", None) or {}).items()):
        for option, value in sorted((options or {}).items()):
            extra_kwargs.append((name, option, python_repr(value)))

    source = source_line(cls) or {}
    return OrderedDict(
        [
            ("python_name", dotted(cls)),
            ("class", cls.__name__),
            ("module", write_module_name(cls.__name__)),
            ("source_file", source.get("file")),
            ("source_line", source.get("line")),
            ("model", model_label(model)),
            ("fields", writable),
            ("read_only", read_only),
            ("extra_kwargs", extra_kwargs),
        ]
    )


def field_spec_literal(field: dict[str, Any]) -> str:
    """A Rust `FieldSpec` expression for one field record."""
    parts = [
        f"name: {rust_string(field['name'])}",
        f"source: {rust_string(field['source'])}",
        f"python_class: {rust_string(field['python_class'])}",
        f"kind: FieldKind::{FIELD_KIND_VARIANTS.get(field['kind'], 'Other')}",
        f"required: {bool_literal(field['required'])}",
        f"write_only: {bool_literal(field['write_only'])}",
        f"allow_null: {bool_literal(field['allow_null'])}",
        f"allow_blank: {bool_literal(field['allow_blank'])}",
        f"allow_empty: {bool_literal(field.get('allow_empty', True))}",
        f"default: {('Some(' + rust_string(field['default']) + ')') if 'default' in field else 'None'}",
        f"max_length: {('Some(' + str(field['max_length']) + ')') if 'max_length' in field else 'None'}",
        f"min_length: {('Some(' + str(field['min_length']) + ')') if 'min_length' in field else 'None'}",
        f"min_value: {('Some(' + str(field['min_value']) + ')') if 'min_value' in field else 'None'}",
        f"max_value: {('Some(' + str(field['max_value']) + ')') if 'max_value' in field else 'None'}",
    ]
    choices = field.get("choices") or []
    rendered_choices = ", ".join(
        f"({rust_string(value)}, {rust_string(display)})" for value, display in choices
    )
    parts.append(f"choices: &[{rendered_choices}]")
    formats = field.get("input_formats") or []
    rendered_formats = ", ".join(rust_string(item) for item in formats)
    parts.append(f"input_formats: &[{rendered_formats}]")
    parts.append(
        f"related_model: {('Some(' + rust_string(field['related_model']) + ')') if field.get('related_model') else 'None'}"
    )
    parts.append(
        f"related_table: {('Some(' + rust_string(field['related_table']) + ')') if field.get('related_table') else 'None'}"
    )
    if "child" in field:
        parts.append(f"child: Some(&{field_spec_literal(field['child'])})")
    else:
        parts.append("child: None")
    return "FieldSpec { " + ", ".join(parts) + " }"


def validate_lines(fields: list[dict[str, Any]]) -> tuple[list[str], list[tuple[str, str, str]]]:
    """`validate` body plus the `(kind, serializer, field)` TODO entries."""
    lines = [
        "pub async fn validate(s: &mut Serializer<'_>, client: &Client) -> Result<(), ViewError> {"
    ]
    todos: list[tuple[str, str, str]] = []
    has_statement = False
    uses_client = False
    for field in fields:
        name = rust_string(field["name"])
        kind = field["kind"]
        if kind == "char":
            has_statement = True
            # A required `CharField` keeps a form `''` and fails `blank`
            # (`rest_framework/fields.py:405-420`); the optional ones treat it
            # as empty.
            helper = "char_field_required" if field["required"] else "char_field"
            lines.append(
                f"    let _ = s.{helper}("
                f"{name}, CharOptions {{ allow_null: {bool_literal(field['allow_null'])}, "
                f"allow_blank: {bool_literal(field['allow_blank'])}, "
                f"trim_whitespace: {bool_literal(field['trim_whitespace'])}, "
                f"max_length: {('Some(' + str(field['max_length']) + ')') if 'max_length' in field else 'None'} }});"
            )
        elif kind == "url" and field.get("url_validators") is not None:
            has_statement = True
            validators = ", ".join(
                f"UrlValidator::{variant}" for variant in field["url_validators"]
            )
            lines.append(
                "    let _ = s.url_field("
                f"{name}, UrlOptions {{ required: {bool_literal(field['required'])}, "
                f"allow_null: {bool_literal(field['allow_null'])}, "
                f"allow_blank: {bool_literal(field['allow_blank'])}, "
                f"trim_whitespace: {bool_literal(field['trim_whitespace'])}, "
                f"max_length: {('Some(' + str(field['max_length']) + ')') if 'max_length' in field else 'None'}, "
                f"validators: &[{validators}] }})?;"
            )
        elif kind == "slug":
            # The chain depends on the model field's declared validators (the
            # restricted slug list) and on its `UniqueValidator` queryset, so
            # only a handler can supply them: `Serializer::slug_field` with a
            # `SlugOptions` built for this serializer.
            lines.append(
                f"    // TODO(slug): {field['name']} needs "
                f"Serializer::slug_field with the model's restricted list and "
                f"UniqueValidator"
            )
            todos.append(("slug", field["serializer"], field["name"]))
        elif kind == "boolean" and not field["allow_null"]:
            has_statement = True
            lines.append(f"    let _ = s.bool_field({name});")
        elif kind == "float":
            has_statement = True
            lines.append(f"    let _ = s.float_field({name}, {bool_literal(field['allow_null'])});")
        elif kind == "datetime":
            formats = field.get("input_formats") or []
            if formats == ["iso-8601"]:
                has_statement = True
                lines.append(
                    f"    let _ = s.datetime_field({name}, {bool_literal(field['allow_null'])});"
                )
            else:
                lines.append(f"    // TODO(datetime-format): {field['name']} accepts {formats!r}")
                todos.append(("datetime-format", field["serializer"], field["name"]))
        elif kind == "json":
            has_statement = True
            lines.append(f"    let _ = s.json_field({name}, {bool_literal(field['allow_null'])});")
        elif kind == "choice" and integer_choices(field):
            has_statement = True
            values = ", ".join(str(value) for value in field["choice_values"])
            lines.append(f"    let _ = s.integer_choice_field({name}, &[{values}]);")
        elif kind == "choice" and string_choices(field):
            has_statement = True
            values = ", ".join(rust_string(value) for value in field["choice_values"])
            lines.append(
                f"    let _ = s.choice_field({name}, {bool_literal(field['allow_null'])}, "
                f"{bool_literal(field['allow_blank'])}, &[{values}]);"
            )
        elif kind == "integer":
            has_statement = True
            lines.append(
                f"    let _ = s.integer_field({name}, {bool_literal(field['allow_null'])}, "
                f"IntegerOptions {{ min_value: {rust_i64_option(field, 'min_value')}, "
                f"max_value: {rust_i64_option(field, 'max_value')} }});"
            )
        elif kind == "date":
            has_statement = True
            formats = field.get("input_formats") or ["iso-8601"]
            rendered = ", ".join(rust_string(item) for item in formats)
            lines.append(
                f"    let _ = s.date_field({name}, {bool_literal(field['allow_null'])}, "
                f"&[{rendered}]);"
            )
        elif kind == "list":
            child = field.get("child") or {}
            table = child.get("related_table") if child.get("kind") == "primary_key" else None
            if child.get("kind") == "uuid":
                # `ListField(child=UUIDField())` — DRF's UUIDField semantics,
                # including its own "Must be a valid UUID." message.
                has_statement = True
                lines.append(f"    let _ = s.uuid_list_field({name});")
            elif table:
                has_statement = True
                uses_client = True
                lines.append(
                    f"    let _ = s.primary_key_list_field(client, {name}, "
                    f"PrimaryKeyListOptions {{ allow_null: {bool_literal(field['allow_null'])}, "
                    f"allow_empty: {bool_literal(field['allow_empty'])}, "
                    f"min_length: {rust_usize_option(field, 'min_length')}, "
                    f"max_length: {rust_usize_option(field, 'max_length')}, "
                    f"query: &{pk_query_const(field)} }}).await?;"
                )
            else:
                todo = "list"
                lines.append(f"    // TODO({todo}): {field['name']}")
                todos.append((todo, field["serializer"], field["name"]))
        elif kind == "many_related" and field.get("related_table"):
            has_statement = True
            uses_client = True
            lines.append(
                f"    let _ = s.primary_key_many_field(client, {name}, "
                f"{bool_literal(field['allow_null'])}, "
                f"{bool_literal(field['allow_empty'])}, "
                f"&{pk_query_const(field)}).await?;"
            )
        elif kind == "uuid":
            has_statement = True
            lines.append(
                f"    let _ = s.field({name}, {bool_literal(field['allow_null'])}, "
                f"fields::uuid_field);"
            )
        elif kind == "primary_key" and field.get("related_table"):
            has_statement = True
            uses_client = True
            lines.append(
                f"    let _ = s.primary_key_field(client, {name}, "
                f"{bool_literal(field['allow_null'])}, &{pk_query_const(field)}).await?;"
            )
        else:
            todo = todo_kind(field)
            lines.append(f"    // TODO({todo}): {field['name']}")
            todos.append((todo, field["serializer"], field["name"]))
        if field["required"]:
            has_statement = True
            lines.append(f"    if s.missing_required({name}) {{")
            lines.append(f"        s.push_error({name}, Detail::required());")
            lines.append("    }")
    if not has_statement:
        lines.append("    let _ = s; // every writable field is a TODO(kind)")
    if not uses_client:
        lines.append("    let _ = client; // no field validates against a table")
    lines.append("    Ok(())")
    lines.append("}")
    return lines, todos


def string_choices(field: dict[str, Any]) -> bool:
    values = field.get("choice_values") or []
    return bool(values) and all(isinstance(value, str) for value in values)


def rust_i64_option(field: dict[str, Any], key: str) -> str:
    value = field.get(key)
    return f"Some({value})" if isinstance(value, int) and not isinstance(value, bool) else "None"


def rust_usize_option(field: dict[str, Any], key: str) -> str:
    value = field.get(key)
    return f"Some({value})" if isinstance(value, int) and not isinstance(value, bool) else "None"


def pk_query_const(field: dict[str, Any]) -> str:
    """The generated `plane_query!` static for one related field."""
    return "PK_" + re.sub(r"[^0-9A-Za-z]+", "_", field["name"]).upper().strip("_")


def pk_query_name(field: dict[str, Any]) -> str:
    """The catalog name of that static (unique across the crate)."""
    return "write_pk_" + write_module_name(field["serializer"]) + "_" + field["name"]


def render_pk_query(field: dict[str, Any]) -> list[str]:
    """The `plane_query!` block for one field's exact DRF pk lookup."""
    query = field["related_query"]
    model = field.get("related_model") or (field.get("child") or {}).get("related_model")
    lines = [
        "plane_db::plane_query! {",
        f"    /// `{field['python_class']}` lookup for `{field['name']}`",
        f"    /// (`{model}`, `{query['manager']}`): DRF runs",
        "    /// `queryset.get(pk=…)` (`rest_framework/relations.py:252-265`);",
        "    /// this is that queryset compiled by Django's SQL compiler, with the",
        "    /// select list replaced by the pk and the ordering dropped. Whatever",
        "    /// the field's manager hides (soft-deleted, triage, archived or draft",
        "    /// rows, depending on the queryset) is DRF's `does_not_exist`, exactly",
        "    /// as in Python.",
        "    ///",
        "    /// Bind order: `$1` the pk.",
        f"    {pk_query_const(field)} = {rust_string(pk_query_name(field))},",
        f"    {rust_string(query['sql'])},",
        '    [("pk", Uuid)]',
        "}",
    ]
    return lines


def integer_choices(field: dict[str, Any]) -> bool:
    values = field.get("choice_values") or []
    return bool(values) and all(
        isinstance(value, int) and not isinstance(value, bool) and -32768 <= value <= 32767
        for value in values
    )


def todo_kind(field: dict[str, Any]) -> str:
    kind = field["kind"]
    if kind == "boolean":
        return "boolean-allow-null"
    if kind == "choice":
        return "choice"
    if kind == "nested_many":
        return "nested"
    return kind


def write_validate_imports(fields: list[dict[str, Any]]) -> list[str]:
    """Import only what this module's `validate` uses, so `-D warnings` holds."""
    names = ["Serializer"]
    if any(field["kind"] == "char" for field in fields):
        names.append("CharOptions")
    if any(field["kind"] == "url" and field.get("url_validators") is not None for field in fields):
        names.append("UrlOptions")
    if any(field.get("url_validators") for field in fields):
        names.append("UrlValidator")
    if any(field["required"] for field in fields):
        names.append("Detail")
    if any(field["kind"] == "integer" for field in fields):
        names.append("IntegerOptions")
    if any(field["kind"] == "uuid" for field in fields):
        # `fields::uuid_field` for a plain DRF `UUIDField`.
        names.append("fields")
    if any(
        field["kind"] == "list"
        and (field.get("child") or {}).get("kind") == "primary_key"
        and (field.get("child") or {}).get("related_table")
        for field in fields
    ):
        names.append("PrimaryKeyListOptions")
    if len(names) == 1:
        return ["use crate::write::Serializer;"]
    return ["use crate::write::{" + ", ".join(sorted(names)) + "};"]


def render_write_module(record: OrderedDict[str, Any]) -> str:
    lines = [
        "// Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "// SPDX-License-Identifier: AGPL-3.0-only",
        "// See the LICENSE file for details.",
        "",
        f"//! Generated from `{record['source_file']}:{record['source_line']}`",
        f"//! (`{record['python_name']}`). Generated by",
        "//! `tools/inventory/serializers.py --rust`; do not edit.",
        "",
        "use deadpool_postgres::Client;",
        "use plane_drf::error::ViewError;",
        "use crate::write::generated::{ExtraKwarg, FieldKind, FieldSpec, ReadOnlySpec};",
        *write_validate_imports(record["fields"]),
        "",
        f"pub const PYTHON_SERIALIZER: &str = {rust_string(record['python_name'])};",
        "",
        "pub const FIELDS: &[FieldSpec] = &[",
    ]
    for field in record["fields"]:
        lines.append(f"    {field_spec_literal(field)},")
    lines.append("];")
    lines.append("")
    lines.append("pub const READ_ONLY_FIELDS: &[ReadOnlySpec] = &[")
    for field in record["read_only"]:
        lines.append(
            "    ReadOnlySpec { "
            f"name: {rust_string(field['name'])}, "
            f"source: {rust_string(field['source'])}, "
            f"python_class: {rust_string(field['python_class'])}, "
            f"kind: FieldKind::{FIELD_KIND_VARIANTS.get(field['kind'], 'Other')}, "
            f"reason: {rust_string(field['reason'])} }},"
        )
    lines.append("];")
    lines.append("")
    lines.append("pub const EXTRA_KWARGS: &[ExtraKwarg] = &[")
    for name, option, value in record["extra_kwargs"]:
        lines.append(
            "    ExtraKwarg { "
            f"name: {rust_string(name)}, option: {rust_string(option)}, value: {rust_string(value)} }},"
        )
    lines.append("];")
    lines.append("")
    for field in record["fields"]:
        field["serializer"] = record["class"]
    for field in record["fields"]:
        if field.get("related_query") is None:
            continue
        lines.extend(render_pk_query(field))
        lines.append("")
    validate, _ = validate_lines(record["fields"])
    lines.extend(validate)
    lines.append("")
    return "\n".join(lines)


def write_generated_mod(records: list[OrderedDict[str, Any]]) -> str:
    lines = [
        "// Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "// SPDX-License-Identifier: AGPL-3.0-only",
        "// See the LICENSE file for details.",
        "",
        "//! Generated serializer write tables.",
        "//!",
        "//! Generated by `apps/server/tools/inventory/serializers.py --rust`; do",
        "//! not edit. Each module describes one DRF serializer's writable fields",
        "//! in validation order, its read-only fields and the `validate` calls a",
        "//! handler would make today. `validate` emits `// TODO(kind)` where the",
        "//! write pipeline has no helper yet; the generator's summary lists them.",
        "",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum FieldKind {",
        "    Char,",
        "    /// `serializers.SlugField`: `Serializer::slug_field`'s validator chain.",
        "    Slug,",
        "    Url,",
        "    Integer,",
        "    Float,",
        "    Boolean,",
        "    Choice,",
        "    DateTime,",
        "    Date,",
        "    Time,",
        "    Uuid,",
        "    PrimaryKey,",
        "    Json,",
        "    List,",
        "    ManyRelated,",
        "    Nested,",
        "    SerializerMethod,",
        "    ModelField,",
        "    Other,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy)]",
        "pub struct FieldSpec {",
        "    pub name: &'static str,",
        "    pub source: &'static str,",
        "    pub python_class: &'static str,",
        "    pub kind: FieldKind,",
        "    pub required: bool,",
        "    pub write_only: bool,",
        "    pub allow_null: bool,",
        "    pub allow_blank: bool,",
        "    pub allow_empty: bool,",
        "    pub default: Option<&'static str>,",
        "    pub max_length: Option<usize>,",
        "    pub min_length: Option<usize>,",
        "    pub min_value: Option<i64>,",
        "    pub max_value: Option<i64>,",
        "    pub choices: &'static [(&'static str, &'static str)],",
        "    pub input_formats: &'static [&'static str],",
        "    pub related_model: Option<&'static str>,",
        "    pub related_table: Option<&'static str>,",
        "    pub child: Option<&'static FieldSpec>,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy)]",
        "pub struct ReadOnlySpec {",
        "    pub name: &'static str,",
        "    pub source: &'static str,",
        "    pub python_class: &'static str,",
        "    pub kind: FieldKind,",
        "    pub reason: &'static str,",
        "}",
        "",
        "#[derive(Debug, Clone, Copy)]",
        "pub struct ExtraKwarg {",
        "    pub name: &'static str,",
        "    pub option: &'static str,",
        "    pub value: &'static str,",
        "}",
        "",
    ]
    for record in sorted(records, key=lambda item: item["module"]):
        lines.append(f"pub mod {record['module']};")
    lines.append("pub mod pk_queries;")
    lines.append("")
    return "\n".join(lines)


def write_generated_pk_catalog(records: list[OrderedDict[str, Any]]) -> str:
    """The aggregated catalog of every generated pk lookup.

    A separate file, not `mod.rs`, because `tests/schema_check.rs` discovers
    catalogs by the `*queries.rs` suffix; naming it that way keeps the
    completeness check for a new generated lookup working without a special
    case.
    """
    lines = [
        "// Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "// SPDX-License-Identifier: AGPL-3.0-only",
        "// See the LICENSE file for details.",
        "",
        "//! Every `plane_db::plane_query!` statement the generated write modules",
        "//! declare, so `tests/schema_check.rs` prepares each one against the real",
        "//! schema. Generated by `tools/inventory/serializers.py --rust`; do not",
        "//! edit.",
        "",
        "/// One statement per related field, in module order.",
        "pub static ALL_QUERIES: &[&plane_db::Query] = &[",
    ]
    for record in sorted(records, key=lambda item: item["module"]):
        for field in record["fields"]:
            if field.get("related_query") is not None:
                lines.append(f"    &super::{record['module']}::{pk_query_const(field)},")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def write_output(
    classes: dict[str, type], requested: list[str]
) -> tuple[dict[Path, str], list[OrderedDict[str, Any]], list[tuple[str, str, str]]]:
    """The generated write modules, the records, and every TODO entry."""
    records = [write_serializer_record(classes[name]) for name in requested]
    files: dict[Path, str] = {}
    todos: list[tuple[str, str, str]] = []
    for record in records:
        for field in record["fields"]:
            field["serializer"] = record["class"]
        _, record_todos = validate_lines(record["fields"])
        todos.extend(record_todos)
        files[WRITE_GENERATED_DIR / f"{record['module']}.rs"] = format_rust(
            render_write_module(record)
        )
    files[WRITE_GENERATED_DIR / "mod.rs"] = format_rust(write_generated_mod(records))
    files[WRITE_GENERATED_DIR / "pk_queries.rs"] = format_rust(
        write_generated_pk_catalog(records)
    )
    return files, records, todos


def select_write_serializers(classes: dict[str, type], spec: str | None) -> list[str]:
    """Resolve `--write-serializers` (class or dotted names) to canonical names."""
    if not spec:
        wanted = list(WRITE_SERIALIZERS)
    else:
        wanted = [item.strip() for item in spec.split(",") if item.strip()]
    resolved: list[str] = []
    for item in wanted:
        if item in classes:
            resolved.append(item)
            continue
        matches = [name for name in classes if name.endswith(f".{item}")]
        if len(matches) == 1:
            resolved.append(matches[0])
            continue
        if not matches:
            raise SystemExit(f"error: no serializer named {item!r}")
        raise SystemExit(
            f"error: serializer {item!r} is ambiguous: {', '.join(sorted(matches))}"
        )
    return resolved


# ---------------------------------------------------------------------------
# Equivalence against hand-written handlers
# ---------------------------------------------------------------------------

HANDLER_CALL_RE = re.compile(
    r"(?:serializer\.(?P<helper>char_field|float_field|bool_field_value|bool_field|"
    r"integer_choice_field|field|datetime_field|json_field)\(|"
    r"related_field\(\s*serializer,\s*client,)"
    r'\s*"(?P<name>[a-z_]+)"'
)


def _enclosing_function(source: str, marker: str) -> str:
    start = source.index(marker)
    body = source[start:]
    following = re.search(r"\n(?:pub )?(?:async )?fn ", body[1:])
    if following:
        body = body[: following.start() + 1]
    return body


def handler_field_records(path: Path, marker: str) -> list[dict[str, Any]]:
    """The field calls a hand-written handler makes, in source order."""
    body = _enclosing_function(path.read_text(encoding="utf-8"), marker)
    records: list[dict[str, Any]] = []
    seen: set[str] = set()
    for match in HANDLER_CALL_RE.finditer(body):
        name = match.group("name")
        if name in seen:
            continue
        seen.add(name)
        helper = match.group("helper") or "related_field"
        record: dict[str, Any] = {"name": name, "helper": helper}
        # The call's own text: up to its closing `);` (a `char_field` options
        # struct can span lines; the first `);` after the name is the end).
        end = body.find(");", match.end())
        window = body[match.end() : end + 2] if end != -1 else ""
        if helper == "char_field":
            record["allow_null"] = "allow_null: true" in window or "nullable()" in window
            record["allow_blank"] = "allow_blank: true" in window or "nullable()" in window
            max_length = re.search(r"with_max_length\((\d+)\)", window) or re.search(
                r"max_length:\s*Some\((\d+)\)", window
            )
            record["max_length"] = int(max_length.group(1)) if max_length else None
        elif helper in ("float_field", "datetime_field", "json_field", "field"):
            value = re.match(r",\s*(true|false)", window)
            if value:
                record["allow_null"] = value.group(1) == "true"
        elif helper == "integer_choice_field":
            values = re.search(r"&\[([0-9,\s]+)\]", window)
            if values:
                record["choices"] = [
                    int(item) for item in values.group(1).replace(" ", "").split(",") if item
                ]
        records.append(record)
    return records


def compare_write_with_handler(
    record: OrderedDict[str, Any], handler: list[dict[str, Any]]
) -> list[str]:
    """Differences between a generated table and a hand-written handler."""
    generated = [field for field in record["fields"] if not field["read_only"]]
    generated_names = [field["name"] for field in generated]
    handler_names = [field["name"] for field in handler]
    problems: list[str] = []
    if generated_names != handler_names:
        missing = [name for name in generated_names if name not in handler_names]
        extra = [name for name in handler_names if name not in generated_names]
        if missing:
            problems.append(f"missing from handler: {', '.join(missing)}")
        if extra:
            problems.append(f"not in DRF's writable fields: {', '.join(extra)}")
        if not missing and not extra:
            problems.append("field order differs")

    by_name = {field["name"]: field for field in handler}
    for field in generated:
        handled = by_name.get(field["name"])
        if handled is None:
            continue
        kind = field["kind"]
        helper = handled["helper"]
        compatible = {
            "char": ("char_field",),
            "float": ("float_field",),
            "datetime": ("datetime_field",),
            "json": ("json_field",),
            # `bool_field_value` is the same helper over a view-injected value.
            "boolean": ("bool_field", "bool_field_value"),
            "choice": ("integer_choice_field", "field"),
            # A custom `field(name, allow_null, parser)` is how the handlers
            # validate integers and string choices today.
            "integer": ("field",),
            "primary_key": ("related_field",),
        }.get(kind)
        if compatible is None or helper not in compatible:
            problems.append(f"{field['name']}: generated {kind}, handler {helper}")
            continue
        if kind == "char":
            for option in ("allow_null", "allow_blank", "max_length"):
                if field.get(option) != handled.get(option):
                    problems.append(
                        f"{field['name']}.{option}: generated {field.get(option)!r}, "
                        f"handler {handled.get(option)!r}"
                    )
        elif kind in ("float", "datetime", "json"):
            if "allow_null" in handled and field["allow_null"] != handled["allow_null"]:
                problems.append(
                    f"{field['name']}.allow_null: generated {field['allow_null']!r}, "
                    f"handler {handled['allow_null']!r}"
                )
        elif kind == "choice" and helper == "integer_choice_field" and "choices" in handled:
            if field.get("choice_values") != handled["choices"]:
                problems.append(
                    f"{field['name']}.choices: generated {field.get('choice_values')!r}, "
                    f"handler {handled['choices']!r}"
                )
    return problems


# ---------------------------------------------------------------------------
# Assembly
# ---------------------------------------------------------------------------


def build_payload(commit: str) -> OrderedDict[str, Any]:
    from rest_framework import serializers as drf

    import rest_framework

    classes = discover_serializers(drf)
    serializers = [serializer_record(cls, name) for name, cls in classes.items()]

    choice_sets = dedupe_choice_sets(serializers)
    routes_payload = json.loads((SERVER_DIR / "routes.json").read_text(encoding="utf-8"))
    views = views_index(routes_payload["routes"])

    import django

    payload: OrderedDict[str, Any] = OrderedDict()
    payload["generated_from"] = OrderedDict(
        [
            ("commit", commit),
            ("django_version", django.get_version()),
            ("drf_version", rest_framework.VERSION),
            ("settings_module", os.environ.get("DJANGO_SETTINGS_MODULE", "")),
            ("modules", list(SERIALIZER_PACKAGES)),
        ]
    )
    payload["expansion_mapper"] = expansion_mapper_record()
    payload["choice_sets"] = choice_sets
    payload["serializers"] = serializers
    payload["views"] = views
    payload["counts"] = OrderedDict(
        [
            ("serializers", len(serializers)),
            (
                "codegen",
                OrderedDict(
                    (kind, sum(1 for record in serializers if record["codegen"] == kind))
                    for kind in ("simple", "dynamic", "v1", "manual")
                ),
            ),
            ("fields", sum(record["field_count"] for record in serializers)),
            ("views", len(views)),
            (
                "views_with_explicit_calls",
                sum(1 for view in views if view.get("calls")),
            ),
        ]
    )
    return payload


def format_rust(source: str) -> str:
    """Run the toolchain's `rustfmt` over generated code.

    The CI check is `cargo fmt --all --check`, so codegen output has to be
    rustfmt-clean on disk. Formatting here (rather than requiring a separate
    `cargo fmt` after every regeneration) keeps `--check` meaningful.
    """
    try:
        result = subprocess.run(
            ["rustfmt", "--edition", "2024"],
            input=source,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError:
        print("warning: rustfmt not found; generated Rust is unformatted", file=sys.stderr)
        return source
    if result.returncode != 0:
        print(
            f"warning: rustfmt failed ({result.stderr.strip()}); leaving output unformatted",
            file=sys.stderr,
        )
        return source
    return result.stdout


def rust_output(payload: OrderedDict[str, Any]) -> dict[Path, str]:
    serializers = payload["serializers"]
    generator = RustGenerator(serializers)
    by_module: dict[str, list[dict[str, Any]]] = {}
    for record in serializers:
        if record["codegen"] == "manual":
            continue
        by_module.setdefault(record["module"], []).append(record)

    files: dict[Path, str] = {}
    mod_lines = [
        "// Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "// SPDX-License-Identifier: AGPL-3.0-only",
        "// See the LICENSE file for details.",
        "",
        "//! Code generated from Django's serializer introspection.",
        "//!",
        "//! `apps/server/tools/inventory/serializers.py --rust` writes one module",
        "//! per Python serializer module. Re-run it after changing `apps/api`; the",
        "//! files are committed so the build does not need Python.",
        "//!",
        "//! Serializers whose `to_representation` is hand-written are **not** here;",
        "//! they are listed as `manual` in `apps/server/SERIALIZERS.md` and get",
        "//! hand-written modules in a later session. A generated struct references",
        "//! a manual nested serializer through `&dyn DrfSerialize`, so a parent",
        "//! compiles before the child's module exists.",
        "",
    ]
    for module in sorted(by_module):
        rust_module = rust_module_name(module)
        mod_lines.append(f"pub mod {rust_module};")
        records = sorted(by_module[module], key=lambda record: record["class"])
        files[GENERATED_DIR / f"{rust_module}.rs"] = format_rust(
            generator.render_module(module, records)
        )
    mod_lines.append("")
    files[GENERATED_DIR / "mod.rs"] = format_rust("\n".join(mod_lines))

    # `serializers.json` (generated) mentions every module name; keep a marker
    # the generator's own self-check can find.
    return files


def write_if_changed(path: Path, content: str, check: bool, changed: list[Path]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    existing = path.read_text(encoding="utf-8") if path.exists() else None
    if existing == content:
        return
    changed.append(path)
    if not check:
        path.write_text(content, encoding="utf-8", newline="\n")


def json_without_commit(text: str) -> str:
    """The inventory with `generated_from.commit` blanked, for freshness checks.

    The commit changes on every commit, so comparing it would report a stale
    inventory even when nothing about the serializers changed.
    """
    payload = json.loads(text)
    payload.setdefault("generated_from", {})["commit"] = ""
    return json.dumps(payload, indent=2, ensure_ascii=False) + "\n"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=SERVER_DIR,
        help="directory for serializers.json and SERIALIZERS.md (default: apps/server)",
    )
    parser.add_argument(
        "--rust-dir",
        type=Path,
        default=GENERATED_DIR,
        help="directory for generated representation modules",
    )
    parser.add_argument(
        "--write-dir",
        type=Path,
        default=WRITE_GENERATED_DIR,
        help="directory for generated write tables",
    )
    parser.add_argument(
        "--write-serializers",
        default=None,
        help=(
            "comma-separated serializer class names to generate write tables for "
            "(default: the work-items set, Sticky and Project)"
        ),
    )
    parser.add_argument(
        "--write-equivalence",
        action="store_true",
        help="compare generated Sticky/Project tables with the hand-written handlers",
    )
    parser.add_argument("--check", action="store_true", help="verify, do not write")
    parser.add_argument(
        "--rust",
        action="store_true",
        help="write the generated Rust modules (the default)",
    )
    parser.add_argument("--skip-rust", action="store_true", help="do not write Rust")
    parser.add_argument(
        "--commit",
        default=None,
        help="record this commit in generated_from instead of `git rev-parse HEAD`",
    )
    args = parser.parse_args(argv)

    setup_django()
    payload = build_payload(args.commit or git_commit())

    changed: list[Path] = []
    json_path = args.out_dir.resolve() / "serializers.json"
    markdown_path = args.out_dir.resolve() / "SERIALIZERS.md"
    json_text = json.dumps(payload, indent=2, ensure_ascii=False) + "\n"
    markdown_text = render_markdown(payload)
    if args.check and json_path.exists():
        if json_without_commit(json_path.read_text(encoding="utf-8")) != json_without_commit(
            json_text
        ):
            changed.append(json_path)
    else:
        write_if_changed(json_path, json_text, args.check, changed)
    write_if_changed(markdown_path, markdown_text, args.check, changed)

    write_records: list[OrderedDict[str, Any]] = []
    write_todos: list[tuple[str, str, str]] = []
    if not args.skip_rust:
        files = rust_output(payload)
        for path, content in files.items():
            target = args.rust_dir.resolve() / path.name
            write_if_changed(target, content, args.check, changed)

        from rest_framework import serializers as drf

        classes = discover_serializers(drf)
        requested = select_write_serializers(classes, args.write_serializers)
        files, write_records, write_todos = write_output(classes, requested)
        for path, content in files.items():
            target = args.write_dir.resolve() / path.name
            write_if_changed(target, content, args.check, changed)

    counts = payload["counts"]
    print(
        f"{counts['serializers']} serializers "
        f"({counts['codegen']['simple']} simple, {counts['codegen']['dynamic']} dynamic, "
        f"{counts['codegen']['v1']} v1, {counts['codegen']['manual']} manual), "
        f"{counts['fields']} fields, {counts['views']} views "
        f"({counts['views_with_explicit_calls']} with explicit serializer calls)"
    )
    if write_records:
        writable = sum(len(record["fields"]) for record in write_records)
        read_only = sum(len(record["read_only"]) for record in write_records)
        print(
            f"write tables: {len(write_records)} serializers, {writable} writable "
            f"fields, {read_only} read-only fields"
        )
        if write_todos:
            grouped: dict[str, list[str]] = {}
            for kind, serializer, field in write_todos:
                grouped.setdefault(kind, []).append(f"{serializer}.{field}")
            print("write TODO kinds:")
            for kind, items in sorted(grouped.items()):
                items = sorted(items)
                shown = ", ".join(items[:6])
                more = f" (+{len(items) - 6} more)" if len(items) > 6 else ""
                print(f"  {kind}: {len(items)} field(s) — {shown}{more}")
        else:
            print("write TODO kinds: none")

    equivalence_failed = False
    if args.write_equivalence:
        by_name = {record["python_name"]: record for record in write_records}
        for python_name, (path, marker) in WRITE_EQUIVALENCE.items():
            record = by_name.get(python_name)
            if record is None:
                print(f"write equivalence {python_name}: not generated, skipped")
                continue
            body = _enclosing_function(path.read_text(encoding="utf-8"), marker)
            if re.search(r"generated::\w+::validate", body):
                print(
                    f"write equivalence {record['class']}: matches "
                    "(the handler calls the generated validate)"
                )
                continue
            problems = compare_write_with_handler(record, handler_field_records(path, marker))
            if problems:
                equivalence_failed = True
                print(f"write equivalence {record['class']}: {len(problems)} mismatch(es)")
                for problem in problems:
                    print(f"  {problem}")
            else:
                print(f"write equivalence {record['class']}: matches the handler")

    if args.check:
        if changed:
            for path in changed:
                print(f"stale: {relative_source(path)}", file=sys.stderr)
            return 1
        if equivalence_failed:
            return 1
        print("inventory is up to date")
        return 0
    for path in changed:
        print(f"wrote {relative_source(path)}")
    return 1 if equivalence_failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
