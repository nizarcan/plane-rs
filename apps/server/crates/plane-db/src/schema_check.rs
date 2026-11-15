// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check`: prepare static statements against a real Django-migrated
//! database and compare the returned column names and Postgres types with the
//! declaration (DECISIONS.md D1).
//!
//! `sqlx` checks queries at compile time; this is the replacement. Every crate
//! keeps its own query catalog (its own module-level `plane_query!`
//! declarations plus an `ALL_QUERIES` slice) and its own integration test that
//! calls [`verify`] with it, so a query that drifts from the schema fails in the
//! crate that owns it instead of in a central list.
//!
//! The skip/require rules live in [`test_database_url`] so every crate's test
//! behaves identically: without `PLANE_RS_TEST_DATABASE_URL` the test prints a
//! notice and passes, and with `PLANE_RS_REQUIRE_DB_TESTS=1` it fails instead,
//! so CI cannot skip the check by accident.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_auth_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_auth_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-auth --test schema_check
//! apps/server/dev/dropdb.sh t_auth_schema
//! ```

use std::fmt;

use crate::catalog::Query;
use crate::url::DatabaseConfig;

/// The variable that names the database to check against.
pub const TEST_DATABASE_URL: &str = "PLANE_RS_TEST_DATABASE_URL";
/// When set to `1`, a missing [`TEST_DATABASE_URL`] fails instead of skipping.
pub const REQUIRE_DB_TESTS: &str = "PLANE_RS_REQUIRE_DB_TESTS";

/// What a successful check covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Statements whose columns matched the declaration.
    pub checked: usize,
    /// `host/name` of the database checked, never the full URL (it carries a
    /// password).
    pub target: String,
}

/// Every statement disagreed with the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub checked: usize,
    pub total: usize,
    /// One line per problem, `"<query name>: <detail>"`.
    pub failures: Vec<String>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "schema_check: {} of {} statements disagree with the database:\n  {}",
            self.failures.len(),
            self.total,
            self.failures.join("\n  ")
        )
    }
}

impl std::error::Error for Error {}

/// Connect to `url` and verify `queries` against it.
///
/// A thin convenience wrapper around [`verify`] for integration tests: it owns
/// the connection task and reports the database name.
pub async fn verify_url(url: &str, queries: &[&Query]) -> Result<Report, VerifyUrlError> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls)
        .await
        .map_err(|error| VerifyUrlError::Connect(error.to_string()))?;
    let connection_task = tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("schema_check connection error: {error}");
        }
    });
    let result = verify(&client, queries).await;
    connection_task.abort();
    let report = result?;
    Ok(Report {
        checked: report.checked,
        target: target_name(url),
    })
}

/// Prepare every statement in `queries` and compare its result columns with the
/// declaration.
///
/// Returns `Err` when at least one statement fails to prepare or disagrees;
/// every failure is collected first, so one run reports all of them.
pub async fn verify(client: &tokio_postgres::Client, queries: &[&Query]) -> Result<Report, Error> {
    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for query in queries {
        let statement = match client.prepare(query.sql).await {
            Ok(statement) => statement,
            Err(error) => {
                let detail = error
                    .as_db_error()
                    .map(|db_error| db_error.message().to_string())
                    .unwrap_or_else(|| error.to_string());
                failures.push(format!("{}: prepare failed: {detail}", query.name));
                continue;
            }
        };
        let actual = statement.columns();
        if actual.len() != query.columns.len() {
            failures.push(format!(
                "{}: expected {} columns, Postgres returned {} ({} vs {})",
                query.name,
                query.columns.len(),
                actual.len(),
                query
                    .columns
                    .iter()
                    .map(|column| column.name)
                    .collect::<Vec<_>>()
                    .join(", "),
                actual
                    .iter()
                    .map(|column| column.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        }
        for (index, expected) in query.columns.iter().enumerate() {
            let column = &actual[index];
            if column.name() != expected.name {
                failures.push(format!(
                    "{}: column {index} is {:?}, declared {:?}",
                    query.name,
                    column.name(),
                    expected.name
                ));
            }
            if column.type_().name() != expected.pg_type.name() {
                failures.push(format!(
                    "{}: column {:?} is {}, declared {}",
                    query.name,
                    expected.name,
                    column.type_().name(),
                    expected.pg_type.name()
                ));
            }
        }
        checked += 1;
    }

    if failures.is_empty() {
        Ok(Report {
            checked,
            target: String::new(),
        })
    } else {
        Err(Error {
            checked,
            total: queries.len(),
            failures,
        })
    }
}

/// The database a run checked, as `host/name`.
fn target_name(url: &str) -> String {
    DatabaseConfig::parse(url)
        .ok()
        .map(|config| {
            let host = config.host.clone().unwrap_or_default();
            let name = config.name.clone().unwrap_or_default();
            format!("{host}/{name}")
        })
        .unwrap_or_else(|| "the configured database".to_string())
}

/// The [`TEST_DATABASE_URL`] rule shared by every crate's schema test.
///
/// * `Ok(Some(url))` — check against `url`.
/// * `Ok(None)` — skip; the caller prints the returned notice.
/// * `Err(message)` — [`REQUIRE_DB_TESTS`] is `1` and no URL was given; the
///   caller must fail with `message`.
pub fn test_database_url() -> Result<Option<String>, String> {
    match std::env::var(TEST_DATABASE_URL)
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(url) => Ok(Some(url)),
        None => {
            if std::env::var(REQUIRE_DB_TESTS).as_deref() == Ok("1") {
                Err(format!(
                    "{REQUIRE_DB_TESTS}=1 but {TEST_DATABASE_URL} is unset; \
                     clone the template first: apps/server/dev/mkdb.sh <name>"
                ))
            } else {
                Ok(None)
            }
        }
    }
}

/// Why [`verify_url`] could not run.
#[derive(Debug)]
pub enum VerifyUrlError {
    Connect(String),
    Check(Error),
}

impl fmt::Display for VerifyUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(message) => write!(f, "schema_check: connect failed: {message}"),
            Self::Check(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for VerifyUrlError {}

impl From<Error> for VerifyUrlError {
    fn from(error: Error) -> Self {
        Self::Check(error)
    }
}
