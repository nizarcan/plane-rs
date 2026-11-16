// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static query catalog (DECISIONS.md D1).
//!
//! `sqlx` would check every statement against the schema at build time. With
//! `tokio-postgres` there is no compile-time check, so every static statement is
//! declared here with the exact result columns it expects, and the
//! `schema_check` integration test prepares each one against a Django-migrated
//! database and compares the names and Postgres types with the declaration.
//!
//! **Registration choice.** Declarations are plain `static` values produced by
//! [`plane_query!`] and aggregated in one `ALL` slice per module
//! (`crate::queries::ALL`). `inventory`/`linkme` would remove the list edit, but
//! they add a linker-section dependency for a problem that does not exist yet:
//! the catalog is small, `plane-db` is the only crate that declares queries, and
//! a missing entry is caught by `catalog_is_complete`, which counts the macro
//! invocations in the source file with `include_str!`. A reviewer sees the list
//! in the same diff as the query.
//!
//! Hot paths fetch result columns by index (`Row::get`), never by name.

/// A Postgres type as `pg_type.typname` (what `tokio_postgres::Type::name()`
/// returns), limited to the types the catalog uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgType {
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Text,
    Varchar,
    Uuid,
    Timestamptz,
    Date,
    Time,
    Json,
    Jsonb,
    Bytea,
    Inet,
    Cidr,
    Interval,
    /// A Postgres array of the wrapped element type (`_uuid`, `_varchar`, …).
    ArrayOf(ArrayElement),
}

/// The element type of a Postgres array.
///
/// Arrays need a separate enum: `PgType` is `Copy` and stored by value in
/// `static` catalog entries, so it cannot contain itself, and `Box` is not
/// usable in a `const` initialiser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayElement {
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Text,
    Varchar,
    Uuid,
    Timestamptz,
    Date,
    Time,
    Json,
    Jsonb,
    Bytea,
    Inet,
    Cidr,
    Interval,
}

impl ArrayElement {
    /// `pg_type.typname` of an array of this element type: Postgres prefixes
    /// the element type's name with `_`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bool => "_bool",
            Self::Int2 => "_int2",
            Self::Int4 => "_int4",
            Self::Int8 => "_int8",
            Self::Float4 => "_float4",
            Self::Float8 => "_float8",
            Self::Numeric => "_numeric",
            Self::Text => "_text",
            Self::Varchar => "_varchar",
            Self::Uuid => "_uuid",
            Self::Timestamptz => "_timestamptz",
            Self::Date => "_date",
            Self::Time => "_time",
            Self::Json => "_json",
            Self::Jsonb => "_jsonb",
            Self::Bytea => "_bytea",
            Self::Inet => "_inet",
            Self::Cidr => "_cidr",
            Self::Interval => "_interval",
        }
    }
}

impl PgType {
    /// The name Postgres reports for the type.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Int2 => "int2",
            Self::Int4 => "int4",
            Self::Int8 => "int8",
            Self::Float4 => "float4",
            Self::Float8 => "float8",
            Self::Numeric => "numeric",
            Self::Text => "text",
            Self::Varchar => "varchar",
            Self::Uuid => "uuid",
            Self::Timestamptz => "timestamptz",
            Self::Date => "date",
            Self::Time => "time",
            Self::Json => "json",
            Self::Jsonb => "jsonb",
            Self::Bytea => "bytea",
            Self::Inet => "inet",
            Self::Cidr => "cidr",
            Self::Interval => "interval",
            Self::ArrayOf(inner) => inner.name(),
        }
    }
}

/// One expected result column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub name: &'static str,
    pub pg_type: PgType,
}

/// A static SQL statement and the result shape it must produce.
#[derive(Debug)]
pub struct Query {
    /// A stable, unique identifier (`load_instance`, `load_session_by_key`, …).
    pub name: &'static str,
    pub sql: &'static str,
    pub columns: &'static [Column],
}

impl Query {
    /// Prepare through the pooled connection's statement cache
    /// (`DECISIONS.md` D1: prefer `prepare_cached`).
    pub async fn prepare(
        &self,
        client: &deadpool_postgres::Client,
    ) -> Result<tokio_postgres::Statement, crate::pool::DbError> {
        Ok(client.prepare_cached(self.sql).await?)
    }

    /// The declared column names, in order.
    pub fn column_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.columns.iter().map(|column| column.name)
    }
}

/// Declare one catalogued statement.
///
/// ```ignore
/// plane_query! {
///     /// Load the single instance row.
///     LOAD_INSTANCE = "load_instance",
///     "SELECT id FROM instances LIMIT 1",
///     [("id", Uuid)],
/// }
/// ```
///
/// A column type is a `PgType` variant name, or `ArrayOf(<ArrayElement>)` for
/// a Postgres array (`PgType` cannot contain itself in a `static`):
///
/// ```ignore
/// plane_query! {
///     MEMBERS = "members",
///     "SELECT ARRAY_AGG(id) AS ids FROM project_members",
///     [("ids", ArrayOf(Uuid))],
/// }
/// ```
#[macro_export]
macro_rules! plane_query {
    (
        $(#[$meta:meta])*
        $ident:ident = $name:literal,
        $sql:literal,
        [ $( ($column:literal, $ty:tt $(($elem:tt))?) ),* $(,)? ]
        $(,)?
    ) => {
        $(#[$meta])*
        pub static $ident: $crate::catalog::Query = $crate::catalog::Query {
            name: $name,
            sql: $sql,
            columns: &[
                $(
                    $crate::catalog::Column {
                        name: $column,
                        pg_type: $crate::plane_pg_type!($ty $(, $elem)?),
                    },
                )*
            ],
        };
    };
}

/// Map one `plane_query!` column type to a [`PgType`](crate::catalog::PgType).
///
/// Split out so the type can be `ArrayOf(Uuid)` (two token trees) without
/// making `plane_query!` a second grammar. Not part of the public API.
#[doc(hidden)]
#[macro_export]
macro_rules! plane_pg_type {
    (ArrayOf, $elem:ident) => {
        $crate::catalog::PgType::ArrayOf($crate::catalog::ArrayElement::$elem)
    };
    ($ty:ident) => {
        $crate::catalog::PgType::$ty
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_type_names() {
        // These are `tokio_postgres::Type::name()` values.
        assert_eq!(PgType::Bool.name(), "bool");
        assert_eq!(PgType::Varchar.name(), "varchar");
        assert_eq!(PgType::Timestamptz.name(), "timestamptz");
        assert_eq!(PgType::Jsonb.name(), "jsonb");
        assert_eq!(PgType::Uuid.name(), "uuid");
        assert_eq!(PgType::ArrayOf(ArrayElement::Uuid).name(), "_uuid");
        assert_eq!(PgType::ArrayOf(ArrayElement::Varchar).name(), "_varchar");
    }

    #[test]
    fn macro_builds_a_query() {
        plane_query! {
            /// Test-only statement.
            TEST_QUERY = "test_query",
            "SELECT 1::int4 AS value",
            [("value", Int4)],
        }
        assert_eq!(TEST_QUERY.name, "test_query");
        assert_eq!(TEST_QUERY.sql, "SELECT 1::int4 AS value");
        assert_eq!(TEST_QUERY.columns.len(), 1);
        assert_eq!(TEST_QUERY.columns[0].name, "value");
        assert_eq!(TEST_QUERY.columns[0].pg_type, PgType::Int4);
    }

    #[test]
    fn macro_accepts_array_columns() {
        plane_query! {
            /// Test-only statement.
            TEST_ARRAY_QUERY = "test_array_query",
            "SELECT ARRAY[]::uuid[] AS ids, ARRAY[]::varchar[] AS names",
            [("ids", ArrayOf(Uuid)), ("names", ArrayOf(Varchar))],
        }
        assert_eq!(
            TEST_ARRAY_QUERY.columns[0].pg_type,
            PgType::ArrayOf(ArrayElement::Uuid)
        );
        assert_eq!(
            TEST_ARRAY_QUERY.columns[1].pg_type,
            PgType::ArrayOf(ArrayElement::Varchar)
        );
        // Mixed scalar and array columns in one declaration.
        plane_query! {
            /// Test-only statement.
            TEST_MIXED_QUERY = "test_mixed_query",
            "SELECT true AS present, ARRAY[]::uuid[] AS ids",
            [("present", Bool), ("ids", ArrayOf(Uuid))],
        }
        assert_eq!(TEST_MIXED_QUERY.columns[0].pg_type, PgType::Bool);
        assert_eq!(
            TEST_MIXED_QUERY.columns[1].pg_type,
            PgType::ArrayOf(ArrayElement::Uuid)
        );
    }
}
