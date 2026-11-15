// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check`: prepare every catalogued statement against a real
//! Django-migrated database and compare the returned column names and Postgres
//! types with the declaration (DECISIONS.md D1).
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_skeleton_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_skeleton_schema \
//!   cargo test -p plane-db --test schema_check
//! apps/server/dev/dropdb.sh t_skeleton_schema
//! ```
//!
//! Without `PLANE_RS_TEST_DATABASE_URL` the test prints a notice and passes.
//! With `PLANE_RS_REQUIRE_DB_TESTS=1` it fails instead, so CI cannot skip the
//! check by accident.

use plane_db::queries::ALL_QUERIES;

#[tokio::test]
async fn schema_check() {
    let Some(url) = std::env::var("PLANE_RS_TEST_DATABASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
    else {
        if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
            panic!(
                "PLANE_RS_REQUIRE_DB_TESTS=1 but PLANE_RS_TEST_DATABASE_URL is unset; \
                 clone the template first: apps/server/dev/mkdb.sh t_skeleton_schema"
            );
        }
        println!(
            "schema_check: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
             set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
        );
        return;
    };

    let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .expect("connect to PLANE_RS_TEST_DATABASE_URL");
    let connection_task = tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("schema_check connection error: {error}");
        }
    });

    let mut checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for query in ALL_QUERIES {
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
    connection_task.abort();

    if !failures.is_empty() {
        panic!(
            "schema_check: {} of {} statements disagree with the database:\n  {}",
            failures.len(),
            ALL_QUERIES.len(),
            failures.join("\n  ")
        );
    }
    // Never print the URL: it usually carries a password.
    let target = plane_db::DatabaseConfig::parse(&url)
        .ok()
        .map(|config| {
            let host = config.host.clone().unwrap_or_default();
            let name = config.name.clone().unwrap_or_default();
            format!("{host}/{name}")
        })
        .unwrap_or_else(|| "the configured database".to_string());
    println!("schema_check: {checked} statements verified against {target}");
}
