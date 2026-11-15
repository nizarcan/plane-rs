// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Database-backed checks for the generated model row types.
//!
//! Three things are verified against a real, Django-migrated database:
//!
//! 1. every generated column exists with the declared Postgres type and
//!    nullability, and the table has no column the row struct is missing;
//! 2. one real row of every populated table decodes through `from_row`
//!    (`instances` and `instance_configurations` are populated by Django's
//!    bootstrap);
//! 3. every manager predicate returns the same ids as Django's manager on the
//!    fixture built by `tools/codegen/manager_fixture.py`.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_models_1
//! cd apps/api
//! DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=fixture \
//!   DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_models_1 \
//!   .venv/bin/python ../server/tools/codegen/manager_fixture.py
//! cd ../server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_models_1 \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-db --test models
//! apps/server/dev/dropdb.sh t_models_1
//! ```
//!
//! The manager test needs the fixture marker row; on a plain clone it prints
//! a notice and returns after the other two tests (which only need the
//! migrated schema from the template).

use std::collections::{BTreeMap, BTreeSet};

use plane_db::models::meta::{MODELS, ModelMeta};
use plane_db::schema_check::test_database_url;
use tokio_postgres::{Client, NoTls};

/// A connected test client, kept alive with the connection task.
struct TestDatabase {
    client: Client,
    _connection: tokio::task::JoinHandle<()>,
}

async fn database() -> Option<TestDatabase> {
    let url = match test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "models: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return None;
        }
        Err(message) => panic!("{message}"),
    };
    let (client, connection) = tokio_postgres::connect(&url, NoTls)
        .await
        .unwrap_or_else(|error| panic!("models: connect failed: {error}"));
    let connection = tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("models: connection error: {error}");
        }
    });
    Some(TestDatabase {
        client,
        _connection: connection,
    })
}

fn quote(identifier: &str) -> String {
    assert!(
        !identifier.contains('"'),
        "unexpected identifier {identifier:?}"
    );
    format!("\"{identifier}\"")
}

fn model_by_label(label: &str) -> &'static ModelMeta {
    MODELS
        .iter()
        .find(|model| model.django_label == label)
        .unwrap_or_else(|| panic!("generated metadata has no model {label}"))
}

#[tokio::test]
async fn columns_types_and_nullability_match_the_database() {
    let Some(database) = database().await else {
        return;
    };

    let rows = database
        .client
        .query(
            "SELECT table_name, column_name, udt_name, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = current_schema()",
            &[],
        )
        .await
        .expect("read information_schema.columns");

    let mut actual: BTreeMap<String, BTreeMap<String, (String, bool)>> = BTreeMap::new();
    for row in rows {
        let table: String = row.get(0);
        let column: String = row.get(1);
        let udt: String = row.get(2);
        let nullable: String = row.get(3);
        actual
            .entry(table)
            .or_default()
            .insert(column, (udt, nullable == "YES"));
    }

    let mut models = 0usize;
    let mut columns = 0usize;
    for model in MODELS {
        let table = actual
            .get(model.table)
            .unwrap_or_else(|| panic!("table {} does not exist", model.table));
        let declared: BTreeSet<&str> = model.columns.iter().map(|column| column.column).collect();
        let found: BTreeSet<&str> = table.keys().map(String::as_str).collect();
        assert_eq!(
            declared, found,
            "{} ({}): generated columns and information_schema.columns disagree",
            model.django_label, model.table
        );
        for column in model.columns {
            let (udt, nullable) = &table[column.column];
            assert_eq!(
                udt,
                column.pg_type.name(),
                "{}.{}: declared Postgres type disagrees with the database",
                model.table,
                column.column
            );
            assert_eq!(
                *nullable, column.nullable,
                "{}.{}: declared nullability disagrees with the database",
                model.table, column.column
            );
            assert_eq!(
                column.rust_type.starts_with("Option<"),
                column.nullable,
                "{}.{}: Option<_> must match nullable",
                model.table,
                column.column
            );
            columns += 1;
        }
        models += 1;
    }

    println!("columns match: {models} models, {columns} columns");
    assert_eq!(models, MODELS.len());
}

#[tokio::test]
async fn one_real_row_per_populated_table_decodes() {
    let Some(database) = database().await else {
        return;
    };

    let mut decoded = Vec::new();
    let mut empty = Vec::new();
    for model in MODELS {
        let select = plane_db::models::with_alias(model.columns_sql, "t");
        let sql = format!("SELECT {select} FROM {} AS t LIMIT 1", quote(model.table));
        match database
            .client
            .query_opt(&sql, &[])
            .await
            .unwrap_or_else(|error| panic!("{}: {error}", model.table))
        {
            Some(row) => {
                (model.decode)(&row, 0)
                    .unwrap_or_else(|error| panic!("{}: decode failed: {error}", model.table));
                decoded.push(model.table);
            }
            None => empty.push(model.table),
        }
    }

    println!(
        "round trip: {} populated tables decoded, {} empty tables skipped",
        decoded.len(),
        empty.len()
    );
    for table in ["instances", "instance_configurations"] {
        assert!(
            decoded.contains(&table),
            "{table} is populated by the bootstrap but did not decode"
        );
    }
}

#[tokio::test]
async fn manager_predicates_match_django() {
    let Some(database) = database().await else {
        return;
    };

    let marker = database
        .client
        .query_opt(
            "SELECT 1 FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
            &[&"models-manager-fixture"],
        )
        .await
        .expect("look for the manager fixture marker");
    if marker.is_none() {
        println!(
            "manager predicates: skipped (database has no models-manager-fixture \
             workspace; run tools/codegen/manager_fixture.py on a t_* clone)"
        );
        return;
    }

    let golden: serde_json::Value = serde_json::from_str(include_str!("golden/managers.json"))
        .expect("parse tests/golden/managers.json");
    let entries = golden["managers"]
        .as_array()
        .expect("golden managers array");
    let mut checked = 0usize;
    let mut covered = BTreeSet::new();
    for entry in entries {
        let label = entry["model"].as_str().expect("model");
        let table = entry["table"].as_str().expect("table");
        let manager_name = entry["manager"].as_str().expect("manager");
        let pk_column = entry["pk_column"].as_str().expect("pk_column");
        let expected: Vec<String> = entry["ids"]
            .as_array()
            .expect("ids")
            .iter()
            .map(|value| value.as_str().expect("id string").to_owned())
            .collect();

        let model = model_by_label(label);
        assert_eq!(model.table, table, "{label}: golden table");
        let manager = model
            .managers
            .iter()
            .find(|manager| manager.name == manager_name)
            .unwrap_or_else(|| panic!("{label}: generated metadata has no manager {manager_name}"));
        let predicate = plane_db::models::with_alias(manager.predicate, "m");
        let sql = format!(
            "SELECT m.{pk}::text FROM {table} AS m WHERE {predicate}",
            pk = quote(pk_column),
            table = quote(table),
        );
        let rows = database
            .client
            .query(&sql, &[])
            .await
            .unwrap_or_else(|error| panic!("{label}.{manager_name}: {error}\n{sql}"));
        let mut actual: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
        let mut expected = expected;
        actual.sort();
        expected.sort();
        assert_eq!(
            actual, expected,
            "{label}.{manager_name}: Rust predicate and Django manager disagree\n{sql}"
        );
        covered.insert((label.to_owned(), manager_name.to_owned()));
        checked += 1;
    }

    let declared: BTreeSet<(String, String)> = MODELS
        .iter()
        .flat_map(|model| {
            model
                .managers
                .iter()
                .map(move |manager| (model.django_label.to_owned(), manager.name.to_owned()))
        })
        .collect();
    assert_eq!(
        covered, declared,
        "tests/golden/managers.json is stale; rerun tools/codegen/manager_fixture.py"
    );
    println!(
        "manager predicates: {checked} managers across {} models match Django",
        MODELS.len()
    );
}

/// `models.json` and the generated `meta::MODELS` are produced by the same run
/// of `tools/codegen/models.py`; this catches a hand edit of either.
#[test]
fn models_json_matches_generated_metadata() {
    let json: serde_json::Value =
        serde_json::from_str(include_str!("../../../models.json")).expect("parse models.json");
    assert_eq!(json["counts"]["models"].as_u64(), Some(MODELS.len() as u64));
    assert_eq!(
        json["counts"]["columns"].as_u64(),
        Some(
            MODELS
                .iter()
                .map(|model| model.columns.len())
                .sum::<usize>() as u64
        )
    );
    assert_eq!(
        json["counts"]["managers"].as_u64(),
        Some(
            MODELS
                .iter()
                .map(|model| model.managers.len())
                .sum::<usize>() as u64
        )
    );
    let models = json["models"].as_array().expect("models array");
    assert_eq!(models.len(), MODELS.len());
    for (entry, model) in models.iter().zip(MODELS) {
        assert_eq!(entry["django_label"].as_str(), Some(model.django_label));
        assert_eq!(entry["django_class"].as_str(), Some(model.django_class));
        assert_eq!(entry["rust_struct"].as_str(), Some(model.rust_struct));
        assert_eq!(entry["db_table"].as_str(), Some(model.table));
        assert_eq!(
            entry["columns"].as_array().map(Vec::len),
            Some(model.columns.len())
        );
        let managers = entry["managers"].as_array().expect("managers array");
        let names: Vec<&str> = managers
            .iter()
            .map(|manager| manager["name"].as_str().expect("manager name"))
            .collect();
        let declared: Vec<&str> = model.managers.iter().map(|manager| manager.name).collect();
        assert_eq!(names, declared, "{}: manager list", model.django_label);
    }
}
