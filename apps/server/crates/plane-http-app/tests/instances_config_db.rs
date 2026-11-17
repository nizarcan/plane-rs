// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Database tests for `PATCH /api/instances/configurations/`'s write
//! (`PUBLIC_INSTANCES.md` §2d): `InstanceConfiguration.objects.bulk_update(
//! values, ["value"], batch_size=100)` writes `value` and nothing else, never
//! inserts, and an empty list performs no statement.
//!
//! `instances/queries.rs` has no route yet (set C registers the handlers), so
//! this executes the two reads and the write against the Django-migrated schema
//! rather than through a scenario:
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_pubp0_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_pubp0_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-http-app --test instances_config_db
//! ```
//!
//! Without `PLANE_RS_TEST_DATABASE_URL` the test prints a notice and passes,
//! like `schema_check`.

use std::str::FromStr;

use deadpool_postgres::Client;
use plane_db::models::license::instance::InstanceConfiguration;
use plane_http_app::instances::{bulk_update_values, queries};
use uuid::Uuid;

async fn client() -> Option<Client> {
    let url = match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "instances_config_db: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return None;
        }
        Err(message) => panic!("{message}"),
    };
    let config = tokio_postgres::Config::from_str(&url).expect("Postgres URL");
    let manager = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(2)
        .build()
        .expect("pool");
    Some(pool.get().await.expect("connection"))
}

/// A configuration row keyed under this test's own prefix, so concurrent runs
/// and the template's seeded rows cannot collide.
async fn insert(
    client: &Client,
    key: &str,
    value: Option<&str>,
    encrypted: bool,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Uuid {
    let id = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO instance_configurations (id, created_at, updated_at, key, value, \
             category, is_encrypted) \
             VALUES ($1, $2, $2, $3, $4, 'TEST', $5)",
            &[&id, &created_at, &key, &value, &encrypted],
        )
        .await
        .expect("insert an instance configuration");
    id
}

async fn delete(client: &Client, ids: &[Uuid]) {
    for id in ids {
        client
            .execute("DELETE FROM instance_configurations WHERE id = $1", &[id])
            .await
            .expect("delete an instance configuration");
    }
}

#[tokio::test]
async fn bulk_update_writes_only_value() {
    let Some(client) = client().await else {
        return;
    };
    let prefix = format!("PUBP0_{}", Uuid::new_v4().simple());
    let older = chrono::Utc::now() - chrono::Duration::seconds(10);
    let newer = chrono::Utc::now();
    // `Meta.ordering = ("-created_at",)`: the newer row must come first.
    let first = insert(&client, &format!("{prefix}_A"), Some("a"), false, older).await;
    let second = insert(&client, &format!("{prefix}_B"), Some("b"), true, newer).await;

    // --- the two reads ---------------------------------------------------
    let statement = queries::CONFIGURATIONS_BY_KEY
        .prepare(&client)
        .await
        .expect("prepare the key lookup");
    let keys = vec![format!("{prefix}_A"), format!("{prefix}_B")];
    let rows = client
        .query(&statement, &[&keys])
        .await
        .expect("select by key");
    let rows: Vec<InstanceConfiguration> = rows
        .iter()
        .map(|row| InstanceConfiguration::from_row(row, 0).expect("decode"))
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, second, "ORDER BY created_at DESC");
    assert_eq!(rows[1].id, first);
    assert_eq!(rows[1].value.as_deref(), Some("a"));
    assert!(rows[0].is_encrypted);

    // An unknown key yields no row, like `filter(key__in=…)`.
    let statement = queries::CONFIGURATIONS_BY_KEY
        .prepare(&client)
        .await
        .expect("prepare");
    assert!(
        client
            .query(&statement, &[&vec![format!("{prefix}_missing")]])
            .await
            .expect("select by key")
            .is_empty()
    );

    // --- the write -------------------------------------------------------
    // `bulk_update([])` returns 0 without a statement; the same call must not
    // need the database at all.
    assert_eq!(bulk_update_values(&client, &[]).await.expect("empty"), 0);

    // `updated_at` before the write: `bulk_update` must not touch it.
    let stamps = [
        (first, updated_at(&client, first).await),
        (second, updated_at(&client, second).await),
    ];

    let updated = bulk_update_values(
        &client,
        &[
            (first, "a2".to_owned()),
            (second, String::new()),
            // An id that does not exist updates nothing instead of inserting.
            (Uuid::new_v4(), "ghost".to_owned()),
        ],
    )
    .await
    .expect("bulk update");
    assert_eq!(updated, 2, "two rows changed, the unknown id inserted none");

    let after_first = load(&client, first).await;
    let after_second = load(&client, second).await;
    assert_eq!(after_first.value.as_deref(), Some("a2"));
    assert_eq!(after_second.value.as_deref(), Some(""));
    assert_eq!(after_second.key, format!("{prefix}_B"));
    assert!(after_second.is_encrypted);
    // Only `value` changed: `bulk_update` bypasses `save()`, so `auto_now`
    // never runs and `updated_at` still holds the insert's value.
    for (id, stamp) in stamps {
        assert_eq!(updated_at(&client, id).await, stamp);
    }

    // --- both reads see every row ---------------------------------------
    let statement = queries::ALL_CONFIGURATIONS
        .prepare(&client)
        .await
        .expect("prepare all");
    let all = client.query(&statement, &[]).await.expect("select all");
    let ours: Vec<&str> = all
        .iter()
        .map(|row| row.get::<_, &str>(6))
        .filter(|key| key.starts_with(&prefix))
        .collect();
    assert_eq!(ours, vec![format!("{prefix}_B"), format!("{prefix}_A")]);

    delete(&client, &[first, second]).await;
}

/// The row's `updated_at`, read back through the same statement the handler
/// uses.
async fn load(client: &Client, id: Uuid) -> InstanceConfiguration {
    let statement = queries::ALL_CONFIGURATIONS
        .prepare(client)
        .await
        .expect("prepare all");
    let rows = client.query(&statement, &[]).await.expect("select all");
    rows.iter()
        .map(|row| InstanceConfiguration::from_row(row, 0).expect("decode"))
        .find(|row| row.id == id)
        .expect("the row exists")
}

/// The row's `updated_at`, read with its own statement so the assertion does
/// not depend on [`load`]'s decode.
async fn updated_at(client: &Client, id: Uuid) -> chrono::DateTime<chrono::Utc> {
    let row = client
        .query_one(
            "SELECT updated_at FROM instance_configurations WHERE id = $1",
            &[&id],
        )
        .await
        .expect("select updated_at");
    row.get(0)
}
