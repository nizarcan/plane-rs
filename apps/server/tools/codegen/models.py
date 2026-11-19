#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the model inventory and the Rust row types for every Django model.

Django is the specification for the Rust rewrite (``RUST_REWRITE.md``). This
script introspects the ``db`` and ``license`` apps with Django itself, so the
generated rows cannot drift from what Django reads and writes:

* ``apps/server/models.json`` -- the machine-readable inventory: one entry per
  concrete model (96) plus the auto-created M2M through tables those models
  define (``users_groups`` and ``users_user_permissions``), with every column
  in ``_meta.concrete_fields`` order, defaults, constraints, managers and the
  exact WHERE predicate each manager applies.
* ``apps/server/crates/plane-db/src/models/**`` -- one Rust module per Django
  model file (``issue.rs``, ``workspace.rs``, ``integration/github.rs``, …)
  containing one struct per model, its select list, manager predicates,
  default ordering and an index-based ``from_row``.

Run it with the already-synced virtualenv::

    cd apps/api
    DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=inventory \\
    DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \\
    .venv/bin/python ../server/tools/codegen/models.py            # models.json
    .venv/bin/python ../server/tools/codegen/models.py --rust     # + Rust
    .venv/bin/python ../server/tools/codegen/models.py --check    # staleness

No database connection is opened: ``field.db_type()`` only needs a
``DatabaseWrapper``, and the manager WHERE clauses are compiled through
Django's SQL compiler without executing anything.

The Rust type mapping is the single table below. ``null=True`` wraps the type
in ``Option<_>``; a ``ForeignKey``/``OneToOneField`` uses the type of its
target field (all targets here are UUID or integer primary keys, plus
``sessions.session_key``).

=====================  =====================================================
Django field           Rust type
=====================  =====================================================
UUIDField              ``Uuid``
CharField              ``String``
TextField              ``String``
SlugField              ``String``
EmailField             ``String``
URLField               ``String``
FileField/ImageField   ``String`` (the stored object key, as Django returns)
DateTimeField          ``DateTime<Utc>``
DateField              ``NaiveDate``
TimeField              ``NaiveTime``
BooleanField           ``bool``
IntegerField           ``i32``
PositiveIntegerField   ``i32``
AutoField              ``i32`` (Django's implicit integer PK)
SmallIntegerField      ``i16``
PositiveSmallIntegerField  ``i16``
BigIntegerField        ``i64``
PositiveBigInteger     ``i64``
BigAutoField           ``i64``
FloatField             ``f64``
DecimalField           ``plane_db::models::Numeric`` (exact ``PyDecimal``)
JSONField              ``plane_db::models::JsonText`` (raw json/jsonb text)
ArrayField             ``Vec<inner>``
BinaryField            ``Vec<u8>``
GenericIPAddressField  ``std::net::IpAddr`` (renders like Django's ``str``)
=====================  =====================================================

``COLUMNS``, manager predicates and ``ORDER_BY`` fragments use the literal
``{alias}`` as a placeholder for the table alias of the query they are used
in; ``plane_db::models::with_alias`` substitutes it.
"""

from __future__ import annotations

import argparse
import inspect
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]
# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(REPO_ROOT / "apps" / "server" / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402
from oracle import display_path as _oracle_display_path  # noqa: E402

API_DIR = _oracle_api_dir()
MODELS_JSON = SERVER_DIR / "models.json"
RUST_MODELS_DIR = SERVER_DIR / "crates" / "plane-db" / "src" / "models"

APPS = ("db", "license")

LICENCE_HEADER = """\
// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details."""

TYPE_MAPPING = (
    ("UUIDField", "Uuid"),
    ("CharField", "String"),
    ("TextField", "String"),
    ("SlugField", "String"),
    ("EmailField", "String"),
    ("URLField", "String"),
    ("FileField", "String"),
    ("ImageField", "String"),
    ("DateTimeField", "DateTime<Utc>"),
    ("DateField", "NaiveDate"),
    ("TimeField", "NaiveTime"),
    ("BooleanField", "bool"),
    ("IntegerField", "i32"),
    ("PositiveIntegerField", "i32"),
    ("AutoField", "i32"),
    ("SmallIntegerField", "i16"),
    ("PositiveSmallIntegerField", "i16"),
    ("BigIntegerField", "i64"),
    ("PositiveBigIntegerField", "i64"),
    ("BigAutoField", "i64"),
    ("FloatField", "f64"),
    ("DecimalField", "Numeric"),
    ("JSONField", "JsonText"),
    ("ArrayField", "Vec<inner>"),
    ("BinaryField", "Vec<u8>"),
    ("GenericIPAddressField", "IpAddr"),
)

# Django type name for a concrete field, through `field.db_type()`, after
# stripping length/precision and normalising to `pg_type.typname` spelling.
PG_TYPES = {
    "uuid": "Uuid",
    "varchar": "Varchar",
    "text": "Text",
    "timestamptz": "Timestamptz",
    "date": "Date",
    "time": "Time",
    "bool": "Bool",
    "int2": "Int2",
    "int4": "Int4",
    "int8": "Int8",
    "float4": "Float4",
    "float8": "Float8",
    "numeric": "Numeric",
    "json": "Json",
    "jsonb": "Jsonb",
    "bytea": "Bytea",
    "inet": "Inet",
    "cidr": "Cidr",
    "interval": "Interval",
}

# `field.db_type()` spellings that differ from `pg_type.typname`.
DB_TYPE_NAMES = {
    "character varying": "varchar",
    "timestamp with time zone": "timestamptz",
    "time without time zone": "time",
    "double precision": "float8",
    "boolean": "bool",
    "integer": "int4",
    "bigint": "int8",
    "smallint": "int2",
}

RUST_KEYWORDS = frozenset(
    """as break const continue crate else enum extern false fn for if impl in
    let loop match mod move mut pub ref return self Self static struct super
    trait true type unsafe use where while async await dyn abstract become box
    do final macro override priv typeof unsized virtual yield try union"""
    .split()
)

# Django's `on_delete` callables, by class name.
ON_DELETE = {
    "CASCADE": "CASCADE",
    "SET_NULL": "SET_NULL",
    "DO_NOTHING": "DO_NOTHING",
    "PROTECT": "PROTECT",
    "RESTRICT": "RESTRICT",
    "SET_DEFAULT": "SET_DEFAULT",
}


class CodegenError(RuntimeError):
    """The Django metadata has a shape this generator does not understand."""


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
# Naming
# ---------------------------------------------------------------------------


def rust_struct_name(class_name: str) -> str:
    """``User_groups`` -> ``UserGroups``; ``APIToken`` stays ``APIToken``."""
    parts = [part for part in class_name.split("_") if part]
    return "".join(part[:1].upper() + part[1:] for part in parts)


def rust_field_name(column: str) -> str:
    """A column name as a Rust field identifier, escaped when needed."""
    return f"r#{column}" if column in RUST_KEYWORDS else column


def rust_const_name(manager_name: str) -> str:
    return manager_name.upper()


def rust_module_path(model: Any) -> str:
    """The generated module path for a model, e.g. ``issue`` or ``license/instance``."""
    module = sys.modules[model.__module__]
    source = Path(module.__file__).resolve()
    for prefix, rust_prefix in (
        (API_DIR / "plane" / "db" / "models", ""),
        (API_DIR / "plane" / "license" / "models", "license/"),
    ):
        try:
            relative = source.relative_to(prefix)
        except ValueError:
            continue
        return rust_prefix + relative.with_suffix("").as_posix()
    raise CodegenError(
        f"{model._meta.label} is defined in {source}, outside plane/db/models "
        "and plane/license/models"
    )


def repo_path(path: Path) -> str:
    # Oracle files render as `apps/api/...`, this checkout's files relative to
    # the repository root: generated sources keep the same citations they had
    # before apps/api moved to the cache.
    return _oracle_display_path(path)


# ---------------------------------------------------------------------------
# Model discovery
# ---------------------------------------------------------------------------


def concrete_models() -> list[Any]:
    """Every concrete db/license model plus its auto-created M2M through tables."""
    from django.apps import apps

    base = [
        model
        for model in apps.get_models()
        if model._meta.app_label in APPS
        and not model._meta.abstract
        and not model._meta.proxy
    ]
    selected = {model._meta.label_lower: model for model in base}
    for model in base:
        for field in model._meta.many_to_many:
            through = field.remote_field.through
            if through._meta.auto_created:
                selected[through._meta.label_lower] = through

    def key(model: Any) -> tuple[str, str, str]:
        return (
            model._meta.app_label,
            rust_module_path(model),
            model.__name__,
        )

    return sorted(selected.values(), key=key)


# ---------------------------------------------------------------------------
# Field metadata
# ---------------------------------------------------------------------------


def field_class(field: Any) -> str:
    return f"{type(field).__module__}.{type(field).__name__}"


def short_class(field: Any) -> str:
    return type(field).__name__


def dotted_name(value: Any) -> str:
    return f"{value.__module__}.{value.__qualname__}"


def json_value(value: Any) -> Any:
    """A JSON-safe scalar for models.json (enum members become their value)."""
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    enum_value = getattr(value, "value", None)
    if enum_value is not None and isinstance(enum_value, (bool, int, float, str)):
        return enum_value
    return str(value)


def default_metadata(field: Any) -> dict[str, Any]:
    from django.db.models import NOT_PROVIDED

    default = field.default
    if default is NOT_PROVIDED:
        return {"kind": "none"}
    if callable(default):
        return {"kind": "callable", "callable": dotted_name(default)}
    return {"kind": "literal", "value": json_value(default)}


def choices_metadata(field: Any) -> list[list[Any]] | None:
    choices = field.choices
    if not choices:
        return None
    return [[json_value(value), str(label)] for value, label in choices]


def rust_type_for(field: Any) -> str:
    """The Rust type for a value field (no ``Option`` wrapper)."""
    from django.contrib.postgres.fields import ArrayField

    if isinstance(field, ArrayField):
        return f"Vec<{rust_type_for(field.base_field)}>"

    name = short_class(field)
    mapping = {
        "UUIDField": "Uuid",
        "CharField": "String",
        "TextField": "String",
        "SlugField": "String",
        "EmailField": "String",
        "URLField": "String",
        "FileField": "String",
        "ImageField": "String",
        "DateTimeField": "DateTime<Utc>",
        "DateField": "NaiveDate",
        "TimeField": "NaiveTime",
        "BooleanField": "bool",
        "IntegerField": "i32",
        "PositiveIntegerField": "i32",
        "AutoField": "i32",
        "SmallIntegerField": "i16",
        "PositiveSmallIntegerField": "i16",
        "BigIntegerField": "i64",
        "PositiveBigIntegerField": "i64",
        "BigAutoField": "i64",
        "FloatField": "f64",
        "DecimalField": "Numeric",
        "JSONField": "JsonText",
        "BinaryField": "Vec<u8>",
        "GenericIPAddressField": "IpAddr",
    }
    if name not in mapping:
        raise CodegenError(f"no Rust type mapping for {field_class(field)}")
    return mapping[name]


def pg_type_for(field: Any, connection: Any) -> tuple[str, str]:
    """``(udt_name, Rust PgType expression)`` for a concrete field."""
    from django.contrib.postgres.fields import ArrayField

    if isinstance(field, ArrayField):
        inner_udt, inner_rust = pg_type_for(field.base_field, connection)
        return f"_{inner_udt}", f"ArrayOf(ArrayElement::{inner_rust})"

    db_type = field.db_type(connection)
    if db_type is None:
        raise CodegenError(f"{field_class(field)} has no database type")

    is_array = bool(re.search(r"\[\d*\]$", db_type))
    base = re.sub(r"\[\d*\]$", "", db_type)
    base = re.sub(r"\(.*?\)$", "", base).strip()
    normalized = DB_TYPE_NAMES.get(base, base)
    if normalized not in PG_TYPES:
        raise CodegenError(f"no Postgres type mapping for {db_type!r} ({field_class(field)})")
    rust_type = PG_TYPES[normalized]
    udt = f"_{normalized}" if is_array else normalized
    return udt, f"ArrayOf(ArrayElement::{rust_type})" if is_array else rust_type


def column_metadata(model: Any, field: Any, connection: Any) -> dict[str, Any]:
    target = field.target_field if field.is_relation else field
    rust_type = rust_type_for(target)
    udt, pg_rust_type = pg_type_for(target, connection)
    result: dict[str, Any] = {
        "field": field.name,
        "attname": field.attname,
        "column": field.column,
        "db_column": field.db_column,
        "field_class": field_class(field),
        "null": bool(field.null),
        "default": default_metadata(field),
        "auto_now": bool(getattr(field, "auto_now", False)),
        "auto_now_add": bool(getattr(field, "auto_now_add", False)),
        "max_length": field.max_length,
        "unique": bool(field.unique),
        "db_index": bool(field.db_index),
        "primary_key": bool(field.primary_key),
        "editable": bool(field.editable),
        "choices": choices_metadata(field),
        "fk": None,
        "pg_type": udt,
        "pg_rust_type": pg_rust_type,
        "rust_type": f"Option<{rust_type}>" if field.null else rust_type,
    }
    if field.is_relation:
        remote = field.remote_field
        result["fk"] = {
            "model": target.model._meta.label,
            "table": target.model._meta.db_table,
            "column": target.column,
            "to_field": target.name,
            "on_delete": ON_DELETE.get(
                getattr(remote.on_delete, "__name__", "?"),
                getattr(remote.on_delete, "__name__", "?"),
            ),
            "related_name": remote.related_name,
            "one_to_one": bool(field.one_to_one),
        }
    return result


# ---------------------------------------------------------------------------
# Manager predicates
# ---------------------------------------------------------------------------


def quote_literal(value: Any) -> str:
    if value is None:
        return "NULL"
    if value is True:
        return "TRUE"
    if value is False:
        return "FALSE"
    if isinstance(value, str):
        return "'" + value.replace("'", "''") + "'"
    if isinstance(value, (int, float)):
        return str(value)
    raise CodegenError(f"cannot render SQL literal {value!r}")


def substitute_params(sql: str, params: list[Any]) -> str:
    if not params:
        return sql
    rendered = tuple(quote_literal(param) for param in params)
    try:
        return sql % rendered
    except TypeError as error:
        raise CodegenError(f"cannot substitute params in {sql!r}: {error}") from error


def leaf_aliases(node: Any) -> set[str]:
    from django.db.models.expressions import Col

    lhs = getattr(node, "lhs", None)
    if isinstance(lhs, Col):
        return {lhs.alias}
    return set()


def node_aliases(node: Any) -> set[str]:
    children = getattr(node, "children", None)
    if children is not None:
        aliases: set[str] = set()
        for child in children:
            aliases |= node_aliases(child)
        return aliases
    return leaf_aliases(node)


def compile_leaf(node: Any, compiler: Any, connection: Any, base_table: str) -> str:
    sql, params = node.as_sql(compiler, connection)
    sql = substitute_params(sql, list(params))
    return sql.replace(f'"{base_table}".', "{alias}.")


def join_condition(join: Any, base_table: str) -> str:
    if join.parent_alias != base_table:
        raise CodegenError(
            f"manager filter joins through {join.parent_alias!r}, not {base_table!r}; "
            "the generator only supports one-hop relation filters"
        )
    comparisons = []
    for parent_field, target_field in join.join_fields:
        comparisons.append(
            f'"{join.table_name}"."{target_field.column}" = '
            f'{{alias}}."{parent_field.column}"'
        )
    if not comparisons:
        raise CodegenError(f"join {join.table_alias!r} has no join fields")
    return " AND ".join(comparisons)


def compile_predicate_node(
    node: Any, query: Any, compiler: Any, connection: Any, base_table: str
) -> str:
    """Translate one WhereNode into a predicate usable on the base table alone.

    Conditions on a forward FK become ``[NOT] EXISTS (SELECT 1 FROM target …)``
    with the join condition Django chose (its ``LEFT OUTER``/``INNER JOIN``).
    That keeps the predicate independent of the caller's FROM clause while
    preserving Django's NULL and "row must exist" semantics.
    """
    from django.db.models.sql.where import WhereNode

    if not isinstance(node, WhereNode):
        return compile_leaf(node, compiler, connection, base_table)

    aliases = node_aliases(node)
    related = aliases - {base_table}
    if not related:
        parts = [
            compile_predicate_node(child, query, compiler, connection, base_table)
            for child in node.children
        ]
        connector = " AND " if node.connector == "AND" else " OR "
        combined = connector.join(f"({part})" for part in parts)
        return f"NOT ({combined})" if node.negated else combined

    if len(related) == 1 and aliases == related:
        related_alias = next(iter(related))
        join = query.alias_map.get(related_alias)
        if join is None:
            raise CodegenError(f"no join metadata for alias {related_alias!r}")
        condition = join_condition(join, base_table)
        parts = []
        for child in node.children:
            if isinstance(child, WhereNode):
                parts.append(
                    compile_predicate_node(child, query, compiler, connection, base_table)
                )
            else:
                sql, params = child.as_sql(compiler, connection)
                parts.append(substitute_params(sql, list(params)))
        connector = " AND " if node.connector == "AND" else " OR "
        inner = connector.join(parts)
        exists = f"EXISTS (SELECT 1 FROM \"{join.table_name}\" WHERE {condition} AND {inner})"
        return f"NOT ({exists})" if node.negated else exists

    if node.negated:
        raise CodegenError(
            "manager filter negates a group that spans several relations; "
            "the generator cannot preserve Django's NULL semantics"
        )
    parts = [
        compile_predicate_node(child, query, compiler, connection, base_table)
        for child in node.children
    ]
    connector = " AND " if node.connector == "AND" else " OR "
    return connector.join(f"({part})" for part in parts)


def compile_plain_node(node: Any, compiler: Any, connection: Any) -> str:
    """Django's WHERE clause as it would run, for the inventory (no EXISTS)."""
    from django.db.models.sql.where import WhereNode

    if not isinstance(node, WhereNode):
        sql, params = node.as_sql(compiler, connection)
        return substitute_params(sql, list(params))
    parts = [compile_plain_node(child, compiler, connection) for child in node.children]
    connector = " AND " if node.connector == "AND" else " OR "
    combined = connector.join(f"({part})" for part in parts)
    return f"NOT ({combined})" if node.negated else combined


def manager_metadata(model: Any) -> tuple[list[dict[str, Any]], str | None]:
    from django.db import DEFAULT_DB_ALIAS, connections

    connection = connections[DEFAULT_DB_ALIAS]
    managers = []
    for manager in model._meta.managers:
        queryset = manager.get_queryset()
        compiler = queryset.query.get_compiler(using=DEFAULT_DB_ALIAS)
        where_node = queryset.query.where
        predicate = compile_predicate_node(
            where_node, queryset.query, compiler, connection, model._meta.db_table
        )
        plain = compile_plain_node(where_node, compiler, connection)
        joins = []
        for alias, join in queryset.query.alias_map.items():
            if alias == model._meta.db_table:
                continue
            joins.append(
                {
                    "alias": alias,
                    "table": join.table_name,
                    "join_type": join.join_type,
                    "parent_alias": join.parent_alias,
                    "columns": [
                        [parent.column, target.column]
                        for parent, target in join.join_fields
                    ],
                }
            )
        managers.append(
            {
                "name": manager.name,
                "class": field_class(manager),
                "default": manager is model._meta.default_manager,
                "django_where": plain or None,
                "django_joins": joins,
                "predicate": predicate or "TRUE",
                "rust_const": rust_const_name(manager.name),
            }
        )

    ordering = ordering_metadata(model)
    return managers, ordering


# ---------------------------------------------------------------------------
# Ordering
# ---------------------------------------------------------------------------


def ordering_metadata(model: Any) -> str | None:
    from django.db import DEFAULT_DB_ALIAS

    queryset = model._base_manager.all()
    compiler = queryset.query.get_compiler(using=DEFAULT_DB_ALIAS)
    items = compiler.get_order_by()
    if not items:
        return None
    fragments = []
    counter = 0
    for _expression, (sql, _params, _is_ref) in items:
        match = re.fullmatch(r'"([^"]+)"\."([^"]+)"( ASC| DESC)?', sql)
        if not match:
            raise CodegenError(f"cannot translate ORDER BY expression {sql!r} for {model._meta.label}")
        table, column, direction = match.group(1), match.group(2), match.group(3) or ""
        if table == model._meta.db_table:
            fragments.append(f'{{alias}}."{column}"{direction}')
            continue
        relation = next(
            (
                field
                for field in model._meta.concrete_fields
                if field.is_relation and field.target_field.model._meta.db_table == table
            ),
            None,
        )
        if relation is None:
            raise CodegenError(
                f"{model._meta.label}: ORDER BY uses {table!r} but no forward FK "
                "points at it"
            )
        counter += 1
        alias = f"{{alias}}_order{counter}"
        fragments.append(
            f'(SELECT {alias}."{column}" FROM "{table}" AS {alias} '
            f'WHERE {alias}."{relation.target_field.column}" = '
            f'{{alias}}."{relation.column}"){direction}'
        )
    return ", ".join(fragments)


# ---------------------------------------------------------------------------
# Constraints
# ---------------------------------------------------------------------------


def render_q(node: Any) -> str:
    from django.db.models import Q

    if isinstance(node, Q):
        parts = [render_q(child) for child in node.children]
        connector = " AND " if node.connector == "AND" else " OR "
        combined = connector.join(parts)
        return f"NOT ({combined})" if node.negated else combined
    lookup, value = node
    return f"{lookup}={value!r}"


def constraints_metadata(model: Any) -> dict[str, Any]:
    from django.db.models import Index, UniqueConstraint

    unique = []
    indexes = []
    for constraint in model._meta.constraints:
        if isinstance(constraint, UniqueConstraint):
            unique.append(
                {
                    "name": constraint.name,
                    "fields": list(constraint.fields),
                    "condition": render_q(constraint.condition)
                    if constraint.condition
                    else None,
                    "nulls_distinct": constraint.nulls_distinct,
                }
            )
        elif isinstance(constraint, Index):
            indexes.append(
                {
                    "name": constraint.name,
                    "fields": list(constraint.fields),
                    "condition": render_q(constraint.condition)
                    if constraint.condition
                    else None,
                }
            )
        else:
            raise CodegenError(f"unsupported constraint {type(constraint).__name__}")
    for index in model._meta.indexes:
        indexes.append(
            {
                "name": index.name,
                "fields": list(index.fields),
                "condition": render_q(index.condition) if index.condition else None,
            }
        )
    return {
        "unique_together": [list(fields) for fields in model._meta.unique_together],
        "unique": unique,
        "indexes": indexes,
    }


# ---------------------------------------------------------------------------
# Model metadata
# ---------------------------------------------------------------------------


def model_metadata(model: Any, connection: Any) -> dict[str, Any]:
    managers, ordering = manager_metadata(model)
    columns = [column_metadata(model, field, connection) for field in model._meta.concrete_fields]
    pk = model._meta.pk
    return {
        "django_label": model._meta.label,
        "django_class": model.__name__,
        "app": model._meta.app_label,
        "module": repo_path(Path(sys.modules[model.__module__].__file__)),
        "rust_module": rust_module_path(model),
        "rust_struct": rust_struct_name(model.__name__),
        "db_table": model._meta.db_table,
        "auto_created": bool(model._meta.auto_created),
        "soft_delete": any(field.name == "deleted_at" for field in model._meta.concrete_fields),
        "primary_key": {
            "field": pk.name,
            "column": pk.column,
            "pg_type": pg_type_for(pk, connection)[0],
            "rust_type": rust_type_for(pk),
        },
        "ordering": list(model._meta.ordering or ()),
        "ordering_sql": ordering,
        "columns": columns,
        "managers": managers,
        "constraints": constraints_metadata(model),
    }


# ---------------------------------------------------------------------------
# Rust rendering
# ---------------------------------------------------------------------------


def rust_string(value: str) -> str:
    escaped = (
        value.replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
    )
    if any(ord(char) < 0x20 for char in value):
        raise CodegenError(f"control character in {value!r}")
    return f'"{escaped}"'


def doc_comment(lines: list[str], indent: str = "") -> list[str]:
    return [f"{indent}/// {line}".rstrip() for line in lines]


def rust_type_imports(models: list[dict[str, Any]]) -> list[str]:
    used: set[str] = set()
    for model in models:
        for column in model["columns"]:
            rust_type = column["rust_type"]
            for token in (
                "DateTime<Utc>",
                "NaiveDate",
                "NaiveTime",
                "Uuid",
                "IpAddr",
                "JsonText",
                "Numeric",
            ):
                if token in rust_type:
                    used.add(token)

    lines = []
    chrono = []
    if "DateTime<Utc>" in used:
        chrono.extend(["DateTime", "Utc"])
    if "NaiveDate" in used:
        chrono.append("NaiveDate")
    if "NaiveTime" in used:
        chrono.append("NaiveTime")
    if chrono:
        chrono.sort()
        lines.append(f"use chrono::{{{', '.join(chrono)}}};")
    if "IpAddr" in used:
        lines.append("use std::net::IpAddr;")
    lines.append("use tokio_postgres::Row;")
    if "Uuid" in used:
        lines.append("use uuid::Uuid;")
    support = ["Error"]
    if "JsonText" in used:
        support.append("JsonText")
    if "Numeric" in used:
        support.append("Numeric")
    lines.append("")
    lines.append(f"use crate::models::{{{', '.join(support)}}};")
    if len(support) == 1:
        lines[-1] = f"use crate::models::{support[0]};"
    return lines


def manager_source(manager: dict[str, Any], model: dict[str, Any]) -> tuple[str, str]:
    """`(file:line, Python source)` of the manager's `get_queryset`."""
    from django.db.models.manager import BaseManager

    try:
        from django.apps import apps

        django_model = apps.get_model(model["django_label"])
        manager_object = getattr(django_model, manager["name"])
        function = type(manager_object).get_queryset
        source_lines, start = inspect.getsourcelines(function)
        source_file = Path(inspect.getsourcefile(function)).resolve()
        location = f"{repo_path(source_file)}:{start}"
        # Django lives in the virtualenv, whose absolute path is not stable
        # between checkouts. Quote it relative to site-packages instead so the
        # generated doc comment is reproducible.
        if "/site-packages/" in location:
            location = "site-packages/" + location.split("/site-packages/", 1)[1]
        return location, "".join(source_lines).rstrip()
    except Exception:  # pragma: no cover - only when Django moves internals
        return (
            f"{manager['class']} (source unavailable)",
            f"# {manager['class']}.get_queryset",
        )


def render_model_struct(model: dict[str, Any]) -> list[str]:
    lines: list[str] = []
    lines.extend(
        doc_comment(
            [
                f"`{model['django_class']}` — table `{model['db_table']}`, "
                f"generated from `{model['module']}`.",
                "",
                f"{len(model['columns'])} columns, "
                + ("soft-deletable." if model["soft_delete"] else "not soft-deletable."),
            ]
        )
    )
    lines.append("#[derive(Debug, Clone, PartialEq)]")
    lines.append(f"pub struct {model['rust_struct']} {{")
    for column in model["columns"]:
        lines.extend(render_field_doc(column, "    "))
        lines.append(f"    pub {rust_field_name(column['column'])}: {column['rust_type']},")
    lines.append("}")
    lines.append("")
    lines.append(f"impl {model['rust_struct']} {{")
    lines.append(f"    /// The table this row is read from (`{model['db_table']}`).")
    lines.append(f"    pub const TABLE: &'static str = {rust_string(model['db_table'])};")
    lines.append("")
    lines.append("    /// The `<alias>`-qualified select list, in `from_row` order.")
    lines.append("    ///")
    lines.append("    /// Replace `{alias}` with the alias of the statement")
    lines.append("    /// (`plane_db::models::with_alias`).")
    lines.append("    pub const COLUMNS: &'static str = concat!(")
    for index, column in enumerate(model["columns"]):
        prefix = "" if index == 0 else ", "
        lines.append(
            f"        {rust_string(prefix + '{alias}.' + quote_identifier(column['column']))},"
        )
    lines.append("    );")
    lines.append("")
    lines.append(f"    /// Number of columns in [`Self::COLUMNS`] ({len(model['columns'])}).")
    lines.append(f"    pub const COLUMN_COUNT: usize = {len(model['columns'])};")
    lines.append("")
    if model["ordering_sql"]:
        lines.extend(
            doc_comment(
                [
                    f"Default ordering (`Meta.ordering = {model['ordering']!r}`).",
                    "",
                    f"Django's SQL: `{model['ordering_sql'].replace('{alias}.', '')}`",
                    "with the alias placeholder inserted.",
                ],
                "    ",
            )
        )
        lines.append(
            f"    pub const ORDER_BY: &'static str = {rust_string(model['ordering_sql'])};"
        )
    else:
        lines.extend(
            doc_comment(
                [
                    "No `Meta.ordering`: Django leaves the row order to Postgres.",
                ],
                "    ",
            )
        )
        lines.append('    pub const ORDER_BY: &\'static str = "";')
    lines.append("")

    for manager in model["managers"]:
        location, source = manager_source(manager, model)
        lines.extend(
            doc_comment(
                [
                    f"Manager `{manager['name']}` — `{manager['class']}.get_queryset`",
                    f"(`{location}`"
                    + (", the default manager)." if manager["default"] else ")."),
                    "",
                    "```python",
                    *source.splitlines(),
                    "```",
                    "",
                    "Django WHERE: "
                    + (f"`{manager['django_where']}`." if manager["django_where"] else "none (every row)."),
                ],
                "    ",
            )
        )
        declaration = (
            f"    pub const {manager['rust_const']}: &'static str = "
            f"{rust_string(manager['predicate'])};"
        )
        value = rust_string(manager["predicate"])
        if len(declaration) <= 100 or 8 + len(value) + 1 > 100:
            lines.append(declaration)
        else:
            lines.append(f"    pub const {manager['rust_const']}: &'static str =")
            lines.append(f"        {value};")
        lines.append("")

    lines.extend(
        doc_comment(
            [
                "Decode this model's row, starting at column `start`.",
                "",
                "Columns are read by index, which is why the select list and this",
                "function are generated together.",
            ],
            "    ",
        )
    )
    lines.append("    pub fn from_row(row: &Row, start: usize) -> Result<Self, Error> {")
    lines.append("        Ok(Self {")
    for index, column in enumerate(model["columns"]):
        offset = "start" if index == 0 else f"start + {index}"
        lines.append(f"            {rust_field_name(column['column'])}: row.try_get({offset})?,")
    lines.append("        })")
    lines.append("    }")
    lines.append("}")
    return lines


def quote_identifier(identifier: str) -> str:
    if '"' in identifier:
        raise CodegenError(f"identifier contains a double quote: {identifier!r}")
    return f'"{identifier}"'


def render_field_doc(column: dict[str, Any], indent: str) -> list[str]:
    name = column["field"]
    if column["fk"]:
        target = column["fk"]
        subject = (
            f"`{name}` → `{target['model']}` (`{target['table']}.{target['column']}`, "
            f"`{target['on_delete']}`"
            + (", one-to-one" if target["one_to_one"] else "")
            + ("), nullable." if column["null"] else ").")
        )
    else:
        details = [column["field_class"].rsplit(".", 1)[-1]]
        if column["max_length"] is not None:
            details.append(f"max_length={column['max_length']}")
        if column["primary_key"]:
            details.append("primary key")
        if column["unique"]:
            details.append("unique")
        if column["db_index"]:
            details.append("db_index")
        if column["auto_now"]:
            details.append("auto_now")
        if column["auto_now_add"]:
            details.append("auto_now_add")
        text = "`, `".join(details)
        subject = f"`{name}` — `{text}`" + (", nullable." if column["null"] else ".")
    default = column["default"]
    if default["kind"] == "callable":
        subject += f" Default: `{default['callable']}`."
    elif default["kind"] == "literal":
        subject += f" Default: `{default['value']!r}`."
    return doc_comment([subject], indent)


def render_rust_file(module: str, models: list[dict[str, Any]]) -> str:
    source_file = models[0]["module"]
    assert all(model["module"] == source_file for model in models)
    lines = [LICENCE_HEADER, ""]
    lines.append(
        f"//! Generated from `{source_file}` by"
        " `apps/server/tools/codegen/models.py --rust`."
    )
    lines.append("//! Never edit this file by hand; regenerate it instead.")
    lines.append("")
    lines.extend(rust_type_imports(models))
    for model in models:
        lines.append("")
        lines.extend(render_model_struct(model))
    lines.append("")
    return "\n".join(lines)


def render_models_mod(models: list[dict[str, Any]]) -> str:
    lines = [LICENCE_HEADER, ""]
    lines.extend(
        [
            "//! Generated row types for every Django model of the `db` and",
            "//! `license` apps.",
            "//!",
            "//! This module is generated by `apps/server/tools/codegen/models.py",
            "//! --rust` from Django's own model metadata (`apps/server/models.json`",
            "//! is the same data in JSON). Never edit it by hand.",
            "//!",
            "//! Every struct has one field per column, in Django's",
            "//! `_meta.concrete_fields` order (the order `COLUMNS` and `from_row`",
            "//! use). The physical Postgres column order can differ for tables that",
            "//! gained columns in later migrations; that does not matter because",
            "//! every statement names its columns explicitly.",
            "//!",
            "//! Type mapping (see `models.py` for the full table):",
            "//!",
            "//! * `Uuid` for `UUIDField`;",
            "//! * `String` for `CharField`, `TextField`, `SlugField`, `EmailField`,",
            "//!   `URLField` and `FileField`;",
            "//! * `DateTime<Utc>`, `NaiveDate`, `NaiveTime` for the temporal fields;",
            "//! * `bool`, `i16`, `i32`, `i64`, `f64` for boolean and numeric fields;",
            "//! * [`Numeric`] for `DecimalField` (an exact `PyDecimal`);",
            "//! * [`JsonText`] for `JSONField` (the raw json/jsonb text);",
            "//! * `Vec<_>` for `ArrayField`, `Vec<u8>` for `BinaryField`;",
            "//! * `std::net::IpAddr` for `GenericIPAddressField`;",
            "//! * `Option<_>` when the column is nullable;",
            "//! * the target field's type for `ForeignKey`/`OneToOneField`.",
            "//!",
            "//! Generated SQL fragments (`COLUMNS`, the manager predicates and",
            "//! `ORDER_BY`) use `{alias}` for the caller's table alias; pass them",
            "//! through [`with_alias`].",
            "",
            "pub mod catalog;",
            "pub mod meta;",
            "mod support;",
            "",
            "pub use support::{Error, JsonText, Numeric, select_list, with_alias};",
        ]
    )

    # Module declarations, grouped by their first path component.
    top_level = sorted(
        {model["rust_module"] for model in models if "/" not in model["rust_module"]}
    )
    nested = sorted(
        {model["rust_module"].split("/")[0] for model in models if "/" in model["rust_module"]}
    )
    lines.append("")
    for name in top_level:
        lines.append(f"pub mod {name};")
    for name in nested:
        lines.append("")
        lines.append(f"pub mod {name} {{")
        children = sorted(
            {
                model["rust_module"].split("/", 1)[1]
                for model in models
                if model["rust_module"].startswith(f"{name}/")
            }
        )
        for child in children:
            lines.append(f"    pub mod {child};")
        lines.append("}")

    # Re-export the structs only (their associated constants come with them).
    lines.append("")
    reexports = [
        f"pub use {model['rust_module'].replace('/', '::')}::{model['rust_struct']};"
        for model in models
    ]
    lines.extend(sorted(reexports))
    lines.append("")
    lines.extend(render_mod_tests())
    lines.append("")
    return "\n".join(lines)


def render_mod_tests() -> list[str]:
    return [
        "#[cfg(test)]",
        "mod generated_tests {",
        "    use std::collections::BTreeSet;",
        "",
        "    use super::{catalog, meta};",
        "",
        "    #[test]",
        "    fn metadata_matches_structs_and_catalog() {",
        "        let mut tables = BTreeSet::new();",
        "        for model in meta::MODELS {",
        "            assert!(",
        "                tables.insert(model.table),",
        '                "duplicate table {} in generated metadata",',
        "                model.table",
        "            );",
        "            assert_eq!(",
        "                model.column_count,",
        "                model.columns.len(),",
        '                "{}: COLUMN_COUNT disagrees with the column list",',
        "                model.django_class",
        "            );",
        "            assert_eq!(",
        "                model.columns_sql.matches(\"{alias}.\").count(),",
        "                model.column_count,",
        '                "{}: COLUMNS has the wrong number of columns",',
        "                model.django_class",
        "            );",
        "            let query_name = format!(\"model_{}\", model.table);",
        "            let query = catalog::ALL_MODEL_QUERIES",
        "                .iter()",
        "                .find(|query| query.name == query_name)",
        "                .unwrap_or_else(|| panic!(\"no catalog entry for {}\", model.table));",
        "            assert_eq!(query.columns.len(), model.columns.len());",
        "            for (declared, column) in query.columns.iter().zip(model.columns) {",
        "                assert_eq!(declared.name, column.column);",
        "                assert_eq!(declared.pg_type, column.pg_type);",
        "            }",
        "            for manager in model.managers {",
        "                assert!(!manager.predicate.is_empty());",
        "            }",
        "            for column in model.columns {",
        "                assert_eq!(",
        "                    column.rust_type.starts_with(\"Option<\"),",
        "                    column.nullable,",
        '                    "{}.{}: Option<_> must match nullable",',
        "                    model.table,",
        "                    column.column",
        "                );",
        "            }",
        "        }",
        "        assert_eq!(tables.len(), meta::MODELS.len());",
        "        assert_eq!(catalog::ALL_MODEL_QUERIES.len(), meta::MODELS.len());",
        "    }",
        "}",
    ]


def render_catalog(models: list[dict[str, Any]]) -> str:
    has_array = any(
        "ArrayOf" in column["pg_rust_type"]
        for model in models
        for column in model["columns"]
    )
    catalog_imports = "ArrayElement, Column, PgType, Query" if has_array else "Column, PgType, Query"
    lines = [LICENCE_HEADER, ""]
    lines.extend(
        [
            "//! Generated query catalog for every model row type.",
            "//!",
            "//! One `SELECT <columns> FROM <table> LIMIT 0` per model, declared with",
            "//! the exact Postgres types the row struct expects. The `schema_check`",
            "//! integration test prepares each of them against a Django-migrated",
            "//! database, so a column that changes type or disappears fails there",
            "//! (DECISIONS.md D1).",
            "",
            f"use crate::catalog::{{{catalog_imports}}};",
        ]
    )
    for model in models:
        upper = re.sub(r"[^A-Za-z0-9]", "_", model["db_table"]).upper()
        lines.append("")
        lines.append(f"static {upper}_COLUMNS: &[Column] = &[")
        for column in model["columns"]:
            lines.append("    Column {")
            lines.append(f'        name: {rust_string(column["column"])},')
            lines.append(f'        pg_type: PgType::{column["pg_rust_type"]},')
            lines.append("    },")
        lines.append("];")
        lines.append("")
        select = ", ".join(
            f'{quote_identifier(model["db_table"])}.{quote_identifier(column["column"])}'
            for column in model["columns"]
        )
        sql = f'SELECT {select} FROM {quote_identifier(model["db_table"])} LIMIT 0'
        lines.append(f"static {upper}_QUERY: Query = Query {{")
        lines.append(f"    name: {rust_string('model_' + model['db_table'])},")
        lines.append(f"    sql: {rust_string(sql)},")
        lines.append(f"    columns: {upper}_COLUMNS,")
        lines.append("};")

    lines.append("")
    lines.append("/// Every generated model statement, in `meta::MODELS` order.")
    lines.append("pub static ALL_MODEL_QUERIES: &[&Query] = &[")
    for model in models:
        upper = re.sub(r"[^A-Za-z0-9]", "_", model["db_table"]).upper()
        lines.append(f"    &{upper}_QUERY,")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


def render_meta(models: list[dict[str, Any]]) -> str:
    has_array = any(
        "ArrayOf" in column["pg_rust_type"]
        for model in models
        for column in model["columns"]
    )
    catalog_imports = "ArrayElement, PgType" if has_array else "PgType"
    lines = [LICENCE_HEADER, ""]
    lines.extend(
        [
            "//! Generated metadata for the database tests.",
            "//!",
            "//! The database tests in `crates/plane-db/tests/models.rs` need the",
            "//! table, columns, Postgres types, nullability, manager predicates and",
            "//! a decoder for every model. Keeping that in typed Rust (rather than",
            "//! parsing `models.json` at test time) lets the compiler check every",
            "//! reference; the JSON file remains the interchange format.",
            "",
            "use super::*;",
            "",
            "use tokio_postgres::Row;",
            "",
            f"use crate::catalog::{{{catalog_imports}}};",
            "",
            "/// A generated row decoder (`Model::from_row` with the value dropped).",
            "pub type Decode = fn(&Row, usize) -> Result<(), Error>;",
            "",
            "/// One column of a model.",
            "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
            "pub struct ColumnMeta {",
            '    /// Django field name (`created_by`).',
            "    pub field: &'static str,",
            '    /// Database column name (`created_by_id`).',
            "    pub column: &'static str,",
            "    pub pg_type: PgType,",
            "    pub nullable: bool,",
            "    /// The generated Rust field type (`Option<Uuid>` for a nullable",
            "    /// column); kept so the tests can check the `Option` rule.",
            "    pub rust_type: &'static str,",
            "}",
            "",
            "/// One manager of a model.",
            "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
            "pub struct ManagerMeta {",
            "    pub name: &'static str,",
            "    /// The generated predicate, with the `{alias}` placeholder.",
            "    pub predicate: &'static str,",
            "}",
            "",
            "/// Everything the database tests need to know about a model.",
            "#[derive(Debug, Clone, Copy)]",
            "pub struct ModelMeta {",
            "    pub django_label: &'static str,",
            "    pub django_class: &'static str,",
            "    pub rust_struct: &'static str,",
            "    pub table: &'static str,",
            "    pub module: &'static str,",
            "    pub primary_key: &'static str,",
            "    pub soft_delete: bool,",
            "    pub column_count: usize,",
            "    pub ordering: &'static str,",
            "    pub columns_sql: &'static str,",
            "    pub columns: &'static [ColumnMeta],",
            "    pub managers: &'static [ManagerMeta],",
            "    pub decode: Decode,",
            "}",
        ]
    )

    for model in models:
        upper = re.sub(r"[^A-Za-z0-9]", "_", model["db_table"]).upper()
        lines.append("")
        lines.append(f"static {upper}_COLUMNS: &[ColumnMeta] = &[")
        for column in model["columns"]:
            lines.append("    ColumnMeta {")
            lines.append(f"        field: {rust_string(column['field'])},")
            lines.append(f"        column: {rust_string(column['column'])},")
            lines.append(f"        pg_type: PgType::{column['pg_rust_type']},")
            lines.append(f"        nullable: {'true' if column['null'] else 'false'},")
            lines.append(f"        rust_type: {rust_string(column['rust_type'])},")
            lines.append("    },")
        lines.append("];")
        lines.append("")
        if len(model["managers"]) == 1:
            manager = model["managers"][0]
            lines.append(f"static {upper}_MANAGERS: &[ManagerMeta] = &[ManagerMeta {{")
            lines.append(f"    name: {rust_string(manager['name'])},")
            lines.append(f"    predicate: {model['rust_struct']}::{manager['rust_const']},")
            lines.append("}];")
        else:
            lines.append(f"static {upper}_MANAGERS: &[ManagerMeta] = &[")
            for manager in model["managers"]:
                lines.append("    ManagerMeta {")
                lines.append(f"        name: {rust_string(manager['name'])},")
                lines.append(f"        predicate: {model['rust_struct']}::{manager['rust_const']},")
                lines.append("    },")
            lines.append("];")
        lines.append("")
        lines.append(
            f"fn decode_{model['db_table']}(row: &Row, start: usize) -> Result<(), Error> {{"
        )
        lines.append(f"    {model['rust_struct']}::from_row(row, start)?;")
        lines.append("    Ok(())")
        lines.append("}")

    lines.append("")
    lines.append("/// Every generated model, in `models.json` order.")
    lines.append("pub static MODELS: &[ModelMeta] = &[")
    for model in models:
        upper = re.sub(r"[^A-Za-z0-9]", "_", model["db_table"]).upper()
        lines.append("    ModelMeta {")
        lines.append(f"        django_label: {rust_string(model['django_label'])},")
        lines.append(f"        django_class: {rust_string(model['django_class'])},")
        lines.append(f"        rust_struct: {rust_string(model['rust_struct'])},")
        lines.append(f"        table: {rust_string(model['db_table'])},")
        lines.append(f"        module: {rust_string(model['rust_module'])},")
        lines.append(f"        primary_key: {rust_string(model['primary_key']['column'])},")
        lines.append(f"        soft_delete: {'true' if model['soft_delete'] else 'false'},")
        lines.append(f"        column_count: {len(model['columns'])},")
        lines.append(f"        ordering: {model['rust_struct']}::ORDER_BY,")
        lines.append(f"        columns_sql: {model['rust_struct']}::COLUMNS,")
        lines.append(f"        columns: {upper}_COLUMNS,")
        lines.append(f"        managers: {upper}_MANAGERS,")
        lines.append(f"        decode: decode_{model['db_table']},")
        lines.append("    },")
    lines.append("];")
    lines.append("")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------


def build_inventory() -> tuple[dict[str, Any], list[dict[str, Any]]]:
    django = setup_django()
    from django.db import DEFAULT_DB_ALIAS, connections

    connection = connections[DEFAULT_DB_ALIAS]
    models = concrete_models()
    metadata = [model_metadata(model, connection) for model in models]

    # The generated names are derived; two Django names that collapse to the
    # same Rust name would produce code that does not compile, or worse, a
    # duplicate schema entry. Fail here with the two Django names instead.
    for key, description in (
        ("rust_struct", "Rust struct"),
        ("db_table", "table"),
    ):
        seen: dict[str, str] = {}
        for model in metadata:
            name = model[key]
            if name in seen:
                raise CodegenError(
                    f"{seen[name]} and {model['django_label']} both map to "
                    f"{description} {name!r}"
                )
            seen[name] = model["django_label"]
    for model in metadata:
        columns: set[str] = set()
        for column in model["columns"]:
            if column["column"] in columns:
                raise CodegenError(
                    f"{model['django_label']} repeats column {column['column']!r}"
                )
            columns.add(column["column"])
        constants = [manager["rust_const"] for manager in model["managers"]]
        if len(set(constants)) != len(constants):
            raise CodegenError(
                f"{model['django_label']} has managers whose names collide: {constants}"
            )

    inventory = {
        "generated_from": {
            "tool": "apps/server/tools/codegen/models.py",
            "django": django.get_version(),
            # Only major.minor: the patch release differs between the local
            # venv and CI, and it does not change the model metadata. A Django
            # bump changes `django` and is meant to fail the check.
            "python": f"{sys.version_info.major}.{sys.version_info.minor}",
        },
        "counts": {
            "models": len(metadata),
            "columns": sum(len(model["columns"]) for model in metadata),
            "managers": sum(len(model["managers"]) for model in metadata),
            "auto_created_models": sum(1 for model in metadata if model["auto_created"]),
        },
        "type_mapping": [
            {"django": django_type, "rust": rust} for django_type, rust in TYPE_MAPPING
        ],
        "models": metadata,
    }
    return inventory, metadata


def generated_files(metadata: list[dict[str, Any]]) -> dict[Path, str]:
    files: dict[Path, str] = {}
    by_module: dict[str, list[dict[str, Any]]] = {}
    for model in metadata:
        by_module.setdefault(model["rust_module"], []).append(model)
    for module, models in sorted(by_module.items()):
        files[RUST_MODELS_DIR / f"{module}.rs"] = render_rust_file(module, models)
    files[RUST_MODELS_DIR / "mod.rs"] = render_models_mod(metadata)
    files[RUST_MODELS_DIR / "catalog.rs"] = render_catalog(metadata)
    files[RUST_MODELS_DIR / "meta.rs"] = render_meta(metadata)
    return files


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--rust",
        action="store_true",
        help="write models.json and the generated Rust row types",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail if models.json or any generated Rust file is stale",
    )
    arguments = parser.parse_args(argv)

    inventory, metadata = build_inventory()
    inventory_text = json.dumps(inventory, indent=2, ensure_ascii=False) + "\n"

    if arguments.check:
        stale: list[str] = []
        if not MODELS_JSON.exists() or MODELS_JSON.read_text(encoding="utf-8") != inventory_text:
            stale.append(repo_path(MODELS_JSON))
        files = generated_files(metadata)
        for path, content in files.items():
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                stale.append(repo_path(path))
        generated = {path.resolve() for path in files}
        for path in sorted(RUST_MODELS_DIR.rglob("*.rs")):
            if path.name == "support.rs":
                continue
            if path.resolve() not in generated:
                stale.append(repo_path(path) + " (no longer generated)")
        if stale:
            print("stale generated model files:", file=sys.stderr)
            for path in stale:
                print(f"  {path}", file=sys.stderr)
            print(
                "run tools/codegen/models.py --rust and commit the result",
                file=sys.stderr,
            )
            return 1
        print(
            f"models.json and {len(files)} Rust files are current "
            f"({inventory['counts']['models']} models, "
            f"{inventory['counts']['columns']} columns)"
        )
        return 0

    write(MODELS_JSON, inventory_text)
    print(f"wrote {repo_path(MODELS_JSON)}")
    if arguments.rust:
        files = generated_files(metadata)
        for path, content in files.items():
            write(path, content)
        print(f"wrote {len(files)} files under {repo_path(RUST_MODELS_DIR)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
