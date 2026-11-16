// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A small SQL composer for the dynamic queries Plane's list endpoints need.
//!
//! Plane's issue list is assembled from user input: every filter combination
//! produces a different set of joins and predicates, so the statements cannot
//! live in a static catalog ([`plane_db::catalog`]). This module is the
//! composer they are built with.
//!
//! ## Shape
//!
//! A statement is a [`Select`]: a projection, a `FROM` table, ordered joins,
//! one predicate, an optional `GROUP BY` and `ORDER BY`, and limit/offset. The
//! predicates and the value lists are [`Frag`]s — SQL text with `?`
//! placeholders and their typed parameters — so fragments compose:
//!
//! ```
//! use plane_query::sql::{Frag, Value};
//!
//! let one = Frag::compare("issues.priority", "=", Value::Text("urgent".into()));
//! let two = Frag::is_null("issues.archived_at");
//! let both = one.and(two);
//! let sql = both.into_sql();
//! assert_eq!(
//!     sql.text,
//!     "(issues.priority = $1) AND (issues.archived_at IS NULL)"
//! );
//! ```
//!
//! Parameters are numbered `$1..$n` once, when the finished fragment is
//! rendered: a fragment can be built, cloned, reordered or embedded in a
//! subquery without renumbering by hand.
//!
//! ## Why not an ORM/query builder crate
//!
//! `sea-query` was considered (it is the one the brief allows). It would have
//! to express the parts that matter here — correlated `ARRAY_AGG` subqueries
//! with `DISTINCT`, `COUNT(DISTINCT …) FILTER (WHERE …)`, window functions
//! partitioned by a user-chosen column, `GROUP BY issues.id` for functional
//! dependency, `AT TIME ZONE` date extraction — and each of those would be
//! either raw SQL passed through it or an impedance mismatch. The statements
//! it *can* express are a small part of the work, and the crate would not
//! remove the hand-written fragments. Hand-writing them keeps the emitted SQL
//! close to what Django emits (which is the specification) and keeps the
//! dependency tree small, which D1 cares about.
//!
//! ## Statement caching: measured, and deliberately not used
//!
//! The brief (D1) says to prefer `prepare_cached`. For this read path the
//! measurement says the opposite, and the crate does not use it for dynamic
//! SQL ([`Sql::query`] sends one unnamed `Parse`/`Bind`/`Execute` exchange):
//!
//! * `deadpool_postgres`'s statement cache is **unbounded per connection** and
//!   evicting an entry never sends `DEALLOCATE`. Every filter combination is a
//!   different SQL text, so the backend accumulates one prepared plan per
//!   request for the life of the connection.
//! * Measured on the dev stack (Postgres cgroup limit 384 MiB): replaying the
//!   1,053-case filter corpus with `prepare_cached` drove the container to
//!   383 MiB and the backend was OOM-killed after ~800 cases; with `prepare`
//!   the same corpus peaks at 57 MiB. That is ~0.4 MiB of backend memory per
//!   distinct statement, which is not recoverable by the client-side cache.
//! * The upside is one round trip per query. Against Django/psycopg2 — which
//!   also sends an unnamed statement per `execute` — the comparison is
//!   therefore like for like, and the measured p50 is in
//!   `crates/plane-query/README.md`.
//!
//! Static statements keep their catalog (`plane-db`) and can use
//! `prepare_cached` safely: their text never changes.

use std::str::FromStr;

use chrono::{DateTime, NaiveDate, Utc};
use tokio_postgres::types::{self, ToSql, Type};
use uuid::Uuid;

/// A typed SQL parameter.
///
/// The variants cover exactly the Postgres types the issue read path binds;
/// `Value` is `ToSql` so it can be handed to `tokio-postgres` unchanged.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Uuid(Uuid),
    UuidArray(Vec<Uuid>),
    Text(String),
    TextArray(Vec<String>),
    Int(i32),
    IntArray(Vec<i32>),
    /// A 64-bit parameter: `LIMIT $1`/`OFFSET $1` and any column Django maps to
    /// `BigIntegerField`. Without it callers had to write `LIMIT $1::int4`.
    Int8(i64),
    /// A `double precision` parameter (`FloatField`/`DecimalField` columns).
    Float8(f64),
    Bool(bool),
    Date(NaiveDate),
    Timestamp(DateTime<Utc>),
    /// A typed SQL `NULL`.
    ///
    /// An unnamed `Parse` has to name every parameter's type, and `NULL` has
    /// no value from which to infer one, so the type travels with it. Prefer
    /// `IS NULL` predicates; this exists for the few statements that bind a
    /// nullable operand positionally.
    Null(Type),
}

impl Value {
    /// The Postgres type this parameter is sent as.
    ///
    /// [`Sql::query`] parses the statement as an **unnamed** one, and an
    /// unnamed `Parse` carries explicit parameter OIDs, so each value has to
    /// name its type. They are the types Django's own bindings resolve to for
    /// the same column comparisons.
    #[must_use]
    pub fn postgres_type(&self) -> Type {
        match self {
            Self::Uuid(_) => Type::UUID,
            Self::UuidArray(_) => Type::UUID_ARRAY,
            Self::Text(_) => Type::VARCHAR,
            Self::TextArray(_) => Type::TEXT_ARRAY,
            Self::Int(_) => Type::INT4,
            Self::IntArray(_) => Type::INT4_ARRAY,
            Self::Int8(_) => Type::INT8,
            Self::Float8(_) => Type::FLOAT8,
            Self::Bool(_) => Type::BOOL,
            Self::Date(_) => Type::DATE,
            Self::Timestamp(_) => Type::TIMESTAMPTZ,
            Self::Null(ty) => ty.clone(),
        }
    }
}

impl ToSql for Value {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut types::private::BytesMut,
    ) -> Result<types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Self::Uuid(value) => value.to_sql(ty, out),
            Self::UuidArray(value) => value.to_sql(ty, out),
            Self::Text(value) => value.to_sql(ty, out),
            Self::TextArray(value) => value.to_sql(ty, out),
            Self::Int(value) => value.to_sql(ty, out),
            Self::IntArray(value) => value.to_sql(ty, out),
            Self::Int8(value) => value.to_sql(ty, out),
            Self::Float8(value) => value.to_sql(ty, out),
            Self::Bool(value) => value.to_sql(ty, out),
            Self::Date(value) => value.to_sql(ty, out),
            Self::Timestamp(value) => value.to_sql(ty, out),
            Self::Null(_) => Ok(types::IsNull::Yes),
        }
    }

    fn accepts(ty: &Type) -> bool {
        Uuid::accepts(ty)
            || Vec::<Uuid>::accepts(ty)
            || String::accepts(ty)
            || Vec::<String>::accepts(ty)
            || i32::accepts(ty)
            || Vec::<i32>::accepts(ty)
            || i64::accepts(ty)
            || f64::accepts(ty)
            || bool::accepts(ty)
            || NaiveDate::accepts(ty)
            || DateTime::<Utc>::accepts(ty)
    }

    fn to_sql_checked(
        &self,
        ty: &Type,
        out: &mut types::private::BytesMut,
    ) -> Result<types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Self::Uuid(value) => value.to_sql_checked(ty, out),
            Self::UuidArray(value) => value.to_sql_checked(ty, out),
            Self::Text(value) => value.to_sql_checked(ty, out),
            Self::TextArray(value) => value.to_sql_checked(ty, out),
            Self::Int(value) => value.to_sql_checked(ty, out),
            Self::IntArray(value) => value.to_sql_checked(ty, out),
            Self::Int8(value) => value.to_sql_checked(ty, out),
            Self::Float8(value) => value.to_sql_checked(ty, out),
            Self::Bool(value) => value.to_sql_checked(ty, out),
            Self::Date(value) => value.to_sql_checked(ty, out),
            Self::Timestamp(value) => value.to_sql_checked(ty, out),
            Self::Null(_) => Ok(types::IsNull::Yes),
        }
    }
}

impl From<Uuid> for Value {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<NaiveDate> for Value {
    fn from(value: NaiveDate) -> Self {
        Self::Date(value)
    }
}

/// A SQL fragment: text with `?` placeholders and the parameters they bind.
///
/// `?` is only ever a placeholder; no fragment contains a literal question
/// mark. [`Frag::into_sql`] replaces them with `$1..$n` in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frag {
    pub sql: String,
    pub params: Vec<Value>,
}

impl Frag {
    /// The empty fragment: true in a `WHERE`, absent from a projection.
    pub fn empty() -> Self {
        Self::default()
    }

    /// SQL text with no parameters.
    pub fn raw(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }

    /// One placeholder bound to `value`.
    pub fn bind(value: impl Into<Value>) -> Self {
        Self {
            sql: "?".to_owned(),
            params: vec![value.into()],
        }
    }

    /// `column IS NULL` / `column IS NOT NULL`.
    pub fn is_null(column: &str) -> Self {
        Self::raw(format!("{column} IS NULL"))
    }

    /// `column IS NOT NULL`.
    pub fn is_not_null(column: &str) -> Self {
        Self::raw(format!("{column} IS NOT NULL"))
    }

    /// `column op ?`.
    pub fn compare(column: &str, op: &str, value: impl Into<Value>) -> Self {
        let mut frag = Self::raw(format!("{column} {op} "));
        frag.push(Self::bind(value));
        frag
    }

    /// `column IN (?, ?, …)`; the empty list has no SQL, callers decide.
    pub fn in_list(column: &str, values: Vec<Value>) -> Self {
        let mut frag = Self::raw(format!("{column} IN ("));
        for (index, value) in values.into_iter().enumerate() {
            if index > 0 {
                frag.push_raw(", ");
            }
            frag.push(Self::bind(value));
        }
        frag.push_raw(")");
        frag
    }

    /// `column IN (?, ?, …)` for a list of UUIDs.
    pub fn in_uuids(column: &str, values: &[Uuid]) -> Self {
        Self::in_list(column, values.iter().copied().map(Value::Uuid).collect())
    }

    /// `column IN (?, ?, …)` for a list of strings.
    pub fn in_texts(column: &str, values: &[String]) -> Self {
        Self::in_list(column, values.iter().cloned().map(Value::Text).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.sql.is_empty()
    }

    pub fn push_raw(&mut self, sql: &str) {
        self.sql.push_str(sql);
    }

    pub fn push(&mut self, mut other: Frag) {
        self.sql.push_str(&other.sql);
        self.params.append(&mut other.params);
    }

    /// Concatenate two fragments.
    pub fn append(mut self, other: Frag) -> Frag {
        self.push(other);
        self
    }

    /// `(a) AND (b)`; an empty side is dropped.
    pub fn and(self, other: Frag) -> Frag {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => Frag::empty(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => {
                let mut frag = self.paren();
                frag.push_raw(" AND ");
                frag.push(other.paren());
                frag
            }
        }
    }

    /// `(a) OR (b)`; an empty side is the identity, matching Django's
    /// `Q() | q` (the empty child is skipped, not treated as `TRUE`).
    pub fn or(self, other: Frag) -> Frag {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => Frag::empty(),
            (true, false) => other,
            (false, true) => self,
            (false, false) => {
                let mut frag = self.paren();
                frag.push_raw(" OR ");
                frag.push(other.paren());
                frag
            }
        }
    }

    /// `NOT (a)`; `NOT` of the empty fragment is the empty fragment.
    pub fn negate(self) -> Frag {
        if self.is_empty() {
            return self;
        }
        let mut frag = Frag::raw("NOT ");
        frag.push(self.paren());
        frag
    }

    /// `(a)`.
    pub fn paren(self) -> Frag {
        if self.is_empty() {
            return self;
        }
        let mut frag = Frag::raw("(");
        frag.push(self);
        frag.push_raw(")");
        frag
    }

    /// `UPPER(column::text) LIKE UPPER(?)`, Django 5.2's `icontains`
    /// (`django/db/backends/postgresql/operations.py:165-182`,
    /// `django/db/models/lookups.py:637-645`).
    pub fn icontains(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("UPPER({column}::text) LIKE UPPER("));
        frag.push(Self::bind(format!("%{}%", prep_for_like_query(needle))));
        frag.push_raw(")");
        frag
    }

    /// `column::text LIKE ?`, Django's `contains`.
    pub fn contains(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("{column}::text LIKE "));
        frag.push(Self::bind(format!("%{}%", prep_for_like_query(needle))));
        frag
    }

    /// `column::text LIKE ?` with `needle%`, Django's `startswith`.
    pub fn startswith(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("{column}::text LIKE "));
        frag.push(Self::bind(format!("{}%", prep_for_like_query(needle))));
        frag
    }

    /// `UPPER(column::text) LIKE UPPER(?)` with `needle%`, Django's
    /// `istartswith`.
    pub fn istartswith(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("UPPER({column}::text) LIKE UPPER("));
        frag.push(Self::bind(format!("{}%", prep_for_like_query(needle))));
        frag.push_raw(")");
        frag
    }

    /// `column::text LIKE ?` with `%needle`, Django's `endswith`.
    pub fn ends_with(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("{column}::text LIKE "));
        frag.push(Self::bind(format!("%{}", prep_for_like_query(needle))));
        frag
    }

    /// `UPPER(column::text) LIKE UPPER(?)` with `%needle`, Django's
    /// `iendswith`.
    pub fn iends_with(column: &str, needle: &str) -> Self {
        let mut frag = Self::raw(format!("UPPER({column}::text) LIKE UPPER("));
        frag.push(Self::bind(format!("%{}", prep_for_like_query(needle))));
        frag.push_raw(")");
        frag
    }

    /// Number the placeholders and hand the statement to `tokio-postgres`.
    pub fn into_sql(self) -> Sql {
        let mut text = String::with_capacity(self.sql.len() + self.params.len() * 4);
        let mut index = 0usize;
        for ch in self.sql.chars() {
            if ch == '?' {
                index += 1;
                text.push('$');
                text.push_str(&index.to_string());
            } else {
                text.push(ch);
            }
        }
        debug_assert_eq!(index, self.params.len(), "placeholder/parameter mismatch");
        Sql {
            text,
            params: self.params,
        }
    }
}

/// `DatabaseOperations.prep_for_like_query`
/// (`django/db/backends/base/operations.py:516-518`, shared by every backend):
/// a LIKE pattern's special characters are escaped before Django wraps the
/// value in `%…%`. Without it `?name=100%` matches every name that contains
/// `100` and `name=_` matches every non-empty name.
pub fn prep_for_like_query(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// A finished statement: `$n` placeholders and their parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Sql {
    pub text: String,
    pub params: Vec<Value>,
}

impl Sql {
    /// Prepare the statement as a **named** server-side statement.
    ///
    /// Prefer [`Sql::query`] for a one-shot dynamic statement. This is for a
    /// caller that reuses the same text with many parameter sets: the `Parse`
    /// is paid once, at the cost of a name (`s0`, `s1`, …) that the connection
    /// never deallocates.
    ///
    /// See the module docs: `prepare_cached` is unbounded per connection here
    /// and leaks backend memory for high-cardinality SQL.
    pub async fn prepare(
        &self,
        client: &deadpool_postgres::Client,
    ) -> Result<tokio_postgres::Statement, tokio_postgres::Error> {
        // Deliberately *not* `prepare_cached`: see the module docs. The cache
        // is unbounded per connection and never sends `DEALLOCATE`, so a
        // high-cardinality workload (every filter combination is a new text)
        // grows the backend until the dev Postgres container OOMs.
        client.prepare(&self.text).await
    }

    /// Run the statement as a genuinely unnamed query.
    ///
    /// `Client::prepare` always allocates a **named** server-side statement
    /// (`s0`, `s1`, …) and is a separate protocol exchange, so it costs a
    /// round trip and a transaction before the execution's own. psycopg2's
    /// `PQexecParams`, which Django uses for every parameterised statement,
    /// parses and executes in one exchange. `Client::query_typed_raw` does the
    /// same (`Parse ""` + `Bind ""` + `Describe` + `Execute` + `Sync` in one
    /// buffer), and [`Value::postgres_type`] supplies the parameter OIDs that
    /// an unnamed parse needs.
    ///
    /// Same rows, same errors, one fewer round trip per statement, and no
    /// server-side statement is retained.
    pub async fn query(
        &self,
        client: &deadpool_postgres::Client,
    ) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
        use futures_util::TryStreamExt;
        let params: Vec<(&(dyn ToSql + Sync), Type)> = self
            .params
            .iter()
            .map(|value| (value as &(dyn ToSql + Sync), value.postgres_type()))
            .collect();
        client
            .query_typed_raw(&self.text, params)
            .await?
            .try_collect()
            .await
    }

    /// Run a statement that returns no rows (`INSERT`/`UPDATE`/`DELETE`).
    ///
    /// The same one-exchange shape as [`Sql::query`] (`execute_typed` sends
    /// the unnamed `Parse` and the `Execute` in one buffer); returns the
    /// affected row count.
    pub async fn execute(
        &self,
        client: &deadpool_postgres::Client,
    ) -> Result<u64, tokio_postgres::Error> {
        let params: Vec<(&(dyn ToSql + Sync), Type)> = self
            .params
            .iter()
            .map(|value| (value as &(dyn ToSql + Sync), value.postgres_type()))
            .collect();
        client.execute_typed(&self.text, &params).await
    }
}

/// How a table is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
}

impl JoinKind {
    fn sql(self) -> &'static str {
        match self {
            Self::Inner => "INNER JOIN",
            Self::Left => "LEFT OUTER JOIN",
        }
    }
}

/// One joined table.
#[derive(Debug, Clone, PartialEq)]
pub struct Join {
    pub kind: JoinKind,
    pub table: &'static str,
    pub alias: &'static str,
    pub on: Frag,
}

impl Join {
    pub fn inner(table: &'static str, alias: &'static str, on: Frag) -> Self {
        Self {
            kind: JoinKind::Inner,
            table,
            alias,
            on,
        }
    }

    pub fn left(table: &'static str, alias: &'static str, on: Frag) -> Self {
        Self {
            kind: JoinKind::Left,
            table,
            alias,
            on,
        }
    }
}

/// A `SELECT` statement under construction.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Select {
    pub distinct: bool,
    pub projection: Vec<Frag>,
    /// The `FROM` target. A [`Frag`] so a derived table can carry parameters.
    pub from: Frag,
    pub joins: Vec<Join>,
    pub predicate: Option<Frag>,
    pub group_by: Vec<String>,
    pub order_by: Vec<Frag>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Select {
    pub fn new(from: impl Into<String>) -> Self {
        Self {
            from: Frag::raw(from.into()),
            ..Self::default()
        }
    }

    /// Start from a derived table (`FROM (<sql>) AS alias`), keeping its
    /// parameters in place. `from` must be an unrendered [`Frag`] (its `?`
    /// placeholders still in place), so the final render numbers every
    /// parameter exactly once.
    pub fn from_frag(from: Frag) -> Self {
        Self {
            from,
            ..Self::default()
        }
    }

    pub fn project(mut self, expression: impl Into<String>) -> Self {
        self.projection.push(Frag::raw(expression));
        self
    }

    pub fn project_frag(mut self, expression: Frag) -> Self {
        self.projection.push(expression);
        self
    }

    pub fn join(mut self, join: Join) -> Self {
        self.joins.push(join);
        self
    }

    pub fn where_(mut self, predicate: Frag) -> Self {
        self.predicate = Some(match self.predicate.take() {
            Some(existing) => existing.and(predicate),
            None => predicate,
        });
        self
    }

    pub fn group_by(mut self, column: impl Into<String>) -> Self {
        self.group_by.push(column.into());
        self
    }

    pub fn order_by(mut self, expression: impl Into<String>) -> Self {
        self.order_by.push(Frag::raw(expression));
        self
    }

    pub fn order_frag(mut self, expression: Frag) -> Self {
        self.order_by.push(expression);
        self
    }

    /// The `SELECT … FROM … WHERE …` part, without ordering or paging.
    fn body(&self, projection: &[Frag]) -> Frag {
        let mut frag = Frag::raw("SELECT ");
        if self.distinct {
            frag.push_raw("DISTINCT ");
        }
        if projection.is_empty() {
            frag.push_raw("1");
        }
        for (index, expression) in projection.iter().enumerate() {
            if index > 0 {
                frag.push_raw(", ");
            }
            frag.push(expression.clone());
        }
        frag.push_raw(" FROM ");
        frag.push(self.from.clone());
        for join in &self.joins {
            frag.push_raw(" ");
            frag.push_raw(join.kind.sql());
            frag.push_raw(" ");
            frag.push_raw(join.table);
            if join.table != join.alias {
                frag.push_raw(" ");
                frag.push_raw(join.alias);
            }
            frag.push_raw(" ON ");
            frag.push(join.on.clone());
        }
        if let Some(predicate) = &self.predicate
            && !predicate.is_empty()
        {
            frag.push_raw(" WHERE ");
            frag.push(predicate.clone());
        }
        if !self.group_by.is_empty() {
            frag.push_raw(" GROUP BY ");
            frag.push_raw(&self.group_by.join(", "));
        }
        frag
    }

    /// The statement as an unrendered fragment, so it can be embedded in a
    /// derived table without renumbering its parameters.
    pub fn to_frag(self) -> Frag {
        let mut frag = self.body(&self.projection);
        for (index, expression) in self.order_by.iter().enumerate() {
            frag.push_raw(if index == 0 { " ORDER BY " } else { ", " });
            frag.push(expression.clone());
        }
        if let Some(limit) = self.limit {
            frag.push_raw(&format!(" LIMIT {limit}"));
        }
        if let Some(offset) = self.offset {
            frag.push_raw(&format!(" OFFSET {offset}"));
        }
        frag
    }

    /// The full statement.
    pub fn into_sql(self) -> Sql {
        self.to_frag().into_sql()
    }

    /// `SELECT count(*) FROM (…)`, the way Django's `QuerySet.count()` wraps a
    /// query: it counts result rows, not distinct entities, unless the inner
    /// query is distinct or grouped.
    ///
    /// * `distinct_key`: `Some("issues.id")` when the queryset had
    ///   `.distinct()` and the count must collapse duplicate join rows.
    /// * `group_key`: `Some("issues.id")` when an aggregate annotation put the
    ///   query in a `GROUP BY`, in which case the count is the number of
    ///   groups.
    pub fn count_frag(&self, distinct_key: Option<&str>, group_key: Option<&str>) -> Frag {
        let projection: Vec<Frag> = match distinct_key {
            Some(key) => vec![Frag::raw(key)],
            None => self.projection.clone(),
        };
        let mut inner = self.clone();
        if distinct_key.is_some() {
            inner.distinct = true;
        }
        inner.order_by.clear();
        inner.limit = None;
        inner.offset = None;
        if let Some(key) = group_key {
            inner.group_by = vec![key.to_owned()];
        }
        let mut frag = Frag::raw("SELECT count(*) FROM (");
        frag.push(inner.body(&projection));
        frag.push_raw(") AS \"__count\"");
        frag
    }

    /// [`Select::count_frag`], rendered.
    pub fn count_sql(&self, distinct_key: Option<&str>, group_key: Option<&str>) -> Sql {
        self.count_frag(distinct_key, group_key).into_sql()
    }
}

/// Parse a UUID the way Django's `UUIDField.to_python` does for a string.
pub fn parse_uuid(value: &str) -> Option<Uuid> {
    Uuid::from_str(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_numbered_in_order() {
        let frag = Frag::compare("a", "=", Value::Int(1))
            .and(Frag::is_null("b"))
            .and(Frag::in_uuids("c", &[Uuid::nil(), Uuid::max()]));
        let sql = frag.into_sql();
        assert_eq!(sql.text, "((a = $1) AND (b IS NULL)) AND (c IN ($2, $3))");
        assert_eq!(sql.params.len(), 3);
    }

    #[test]
    fn empty_fragments_disappear() {
        assert!(Frag::empty().and(Frag::empty()).is_empty());
        assert_eq!(Frag::empty().and(Frag::raw("x")).sql, "x");
        assert!(Frag::is_null("a").negate().sql.starts_with("NOT ("));
        assert_eq!(Frag::empty().negate().sql, "");
        assert_eq!(Frag::empty().or(Frag::raw("x")).sql, "x");
        assert_eq!(Frag::raw("x").or(Frag::empty()).sql, "x");
        assert!(Frag::empty().or(Frag::empty()).is_empty());
    }

    #[test]
    fn like_needles_are_escaped_like_django() {
        // `DatabaseOperations.prep_for_like_query`: `\\` -> `\\\\`, `%` -> `\\%`,
        // `_` -> `\\_` (django/db/backends/base/operations.py:516-518).
        assert_eq!(prep_for_like_query("plain"), "plain");
        assert_eq!(prep_for_like_query("100%"), "100\\%");
        assert_eq!(prep_for_like_query("a_b"), "a\\_b");
        assert_eq!(prep_for_like_query("a\\b"), "a\\\\b");
        assert_eq!(prep_for_like_query("\\%_"), "\\\\\\%\\_");

        let sql = Frag::icontains("issues.name", "100%").into_sql();
        assert_eq!(
            sql.params,
            vec![Value::Text("%100\\%%".to_owned())],
            "{:?}",
            sql.params
        );
        let sql = Frag::startswith("issues.name", "_x").into_sql();
        assert_eq!(sql.params, vec![Value::Text("\\_x%".to_owned())]);
        let sql = Frag::ends_with("issues.name", "a\\").into_sql();
        assert_eq!(sql.params, vec![Value::Text("%a\\\\".to_owned())]);
    }

    #[test]
    fn icontains_matches_django_shape() {
        let sql = Frag::icontains("issues.name", "abc").into_sql();
        assert_eq!(sql.text, "UPPER(issues.name::text) LIKE UPPER($1)");
        assert_eq!(sql.params, vec![Value::Text("%abc%".to_owned())]);
    }

    #[test]
    fn count_wraps_the_inner_query() {
        let select = Select::new("issues")
            .project("issues.id")
            .where_(Frag::compare(
                "issues.priority",
                "=",
                Value::Text("urgent".into()),
            ));
        let sql = select.count_sql(Some("issues.id"), None);
        assert_eq!(
            sql.text,
            "SELECT count(*) FROM (SELECT DISTINCT issues.id FROM issues WHERE issues.priority = $1) AS \"__count\""
        );
        // Without a `distinct_key` the projection is kept (the caller may need
        // a window expression in it, as the grouped count does).
        let sql = select.count_sql(None, None);
        assert_eq!(
            sql.text,
            "SELECT count(*) FROM (SELECT issues.id FROM issues WHERE issues.priority = $1) AS \"__count\""
        );
    }

    #[test]
    fn from_frag_keeps_parameters() {
        let inner = Frag::raw("SELECT 1 WHERE x = ").append(Frag::bind(Value::Int(7)));
        let outer = Select::from_frag(Frag {
            sql: format!("({}) AS inner", inner.sql),
            params: inner.params,
        })
        .project("inner.*")
        .where_(Frag::compare("y", "=", Value::Int(8)));
        let sql = outer.into_sql();
        assert_eq!(
            sql.text,
            "SELECT inner.* FROM (SELECT 1 WHERE x = $1) AS inner WHERE y = $2"
        );
        assert_eq!(sql.params, vec![Value::Int(7), Value::Int(8)]);
    }

    #[test]
    fn leading_limit_and_offset_follow_the_order() {
        let mut select = Select::new("issues").project("issues.id");
        select.limit = Some(10);
        select.offset = Some(20);
        let sql = select.into_sql();
        assert_eq!(sql.text, "SELECT issues.id FROM issues LIMIT 10 OFFSET 20");
    }

    #[test]
    fn postgres_type_covers_every_variant() {
        // The OIDs an unnamed `Parse` has to carry. Arrays keep their element
        // type; `Int8` is `LIMIT`/`OFFSET` and bigint columns; a typed `NULL`
        // carries the column's type (or `UNKNOWN` to let the server infer).
        let cases = [
            (Value::Uuid(Uuid::nil()), Type::UUID),
            (Value::UuidArray(vec![Uuid::nil()]), Type::UUID_ARRAY),
            (Value::Text("x".to_owned()), Type::VARCHAR),
            (Value::TextArray(vec!["x".to_owned()]), Type::TEXT_ARRAY),
            (Value::Int(1), Type::INT4),
            (Value::IntArray(vec![1, 2]), Type::INT4_ARRAY),
            (Value::Int8(1), Type::INT8),
            (Value::Float8(1.5), Type::FLOAT8),
            (Value::Bool(true), Type::BOOL),
            (
                Value::Date(NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()),
                Type::DATE,
            ),
            (
                Value::Timestamp(DateTime::<Utc>::from_timestamp(0, 0).unwrap()),
                Type::TIMESTAMPTZ,
            ),
            (Value::Null(Type::UUID), Type::UUID),
            (Value::Null(Type::UNKNOWN), Type::UNKNOWN),
        ];
        for (value, expected) in cases {
            assert_eq!(value.postgres_type(), expected, "{value:?}");
        }
    }

    #[test]
    fn null_binds_as_sql_null() {
        let mut buf = types::private::BytesMut::new();
        let null = Value::Null(Type::UUID).to_sql_checked(&Type::UUID, &mut buf);
        assert!(matches!(null, Ok(types::IsNull::Yes)));
        let value = Value::Uuid(Uuid::nil()).to_sql_checked(&Type::UUID, &mut buf);
        assert!(matches!(value, Ok(types::IsNull::No)));
    }
}
