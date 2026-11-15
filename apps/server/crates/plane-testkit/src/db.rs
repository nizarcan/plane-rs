// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Database cloning, catalog introspection and step snapshots.
//!
//! A snapshot is one query per snapshot, not one per table: every table is
//! appended to a single `UNION ALL` of `SELECT '<table>', to_jsonb(t)::text`,
//! sent with `simple_query` so the result columns are plain text. On the
//! ~90-table Plane schema this is a single round trip and a few milliseconds.
//!
//! Rows are keyed by their primary key from `information_schema`, so a step
//! diff is "inserted / deleted / updated" per table instead of a whole-table
//! comparison.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tokio_postgres::{Client, NoTls, SimpleQueryMessage};

use crate::util::validate_identifier;

#[derive(Debug, Clone)]
pub struct TableMeta {
    pub schema: String,
    pub name: String,
    pub pk_columns: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub tables: Vec<TableMeta>,
}

impl Catalog {
    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DbSnapshot {
    /// Key: `schema.table`. Value: primary-key → row JSON.
    pub tables: BTreeMap<String, BTreeMap<String, Value>>,
}

impl DbSnapshot {
    pub fn row_count(&self) -> usize {
        self.tables.values().map(BTreeMap::len).sum()
    }

    pub fn table_row_count(&self, table: &str) -> usize {
        self.tables.get(table).map_or(0, BTreeMap::len)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TableDiff {
    pub inserted: Vec<Value>,
    pub deleted: Vec<Value>,
    /// `{"pk": [...], "changed": {column: {"a": ..., "b": ...}}}`
    pub updated: Vec<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DbDiff {
    pub tables: BTreeMap<String, TableDiff>,
}

impl DbDiff {
    pub fn is_empty(&self) -> bool {
        self.tables.values().all(|table| {
            table.inserted.is_empty() && table.deleted.is_empty() && table.updated.is_empty()
        })
    }

    pub fn total_changes(&self) -> usize {
        self.tables
            .values()
            .map(|table| table.inserted.len() + table.deleted.len() + table.updated.len())
            .sum()
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotResult {
    pub snapshot: DbSnapshot,
    pub duration: Duration,
}

pub struct DbAdmin {
    url: String,
}

impl DbAdmin {
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    fn config(&self, dbname: &str) -> Result<tokio_postgres::Config> {
        let mut config = tokio_postgres::Config::from_str(&self.url)
            .with_context(|| format!("parsing Postgres URL {:?}", self.url))?;
        config.dbname(dbname);
        Ok(config)
    }

    pub async fn connect(&self, dbname: &str) -> Result<Client> {
        let config = self.config(dbname)?;
        let (client, connection) = config
            .connect(NoTls)
            .await
            .with_context(|| format!("connecting to Postgres database {dbname:?}"))?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::debug!("Postgres connection closed: {error}");
            }
        });
        Ok(client)
    }

    /// Database used for `CREATE DATABASE` / `DROP DATABASE` statements.
    fn maintenance_db(&self) -> String {
        tokio_postgres::Config::from_str(&self.url)
            .ok()
            .and_then(|config| config.get_dbname().map(str::to_string))
            .unwrap_or_else(|| "postgres".to_string())
    }

    pub async fn database_exists(&self, name: &str) -> Result<bool> {
        validate_identifier(name)?;
        let client = self.connect(&self.maintenance_db()).await?;
        let row = client
            .query_opt("SELECT 1 FROM pg_database WHERE datname = $1", &[&name])
            .await
            .context("querying pg_database")?;
        Ok(row.is_some())
    }

    /// Clone `template` into `target`, replacing any previous `target`.
    ///
    /// `CREATE DATABASE ... TEMPLATE ...` fails while the template has active
    /// connections. The dev stack keeps the template idle, but a concurrent
    /// agent may briefly hold one, so the call retries a few times.
    pub async fn clone_database(&self, template: &str, target: &str) -> Result<()> {
        validate_identifier(template)?;
        validate_identifier(target)?;
        let client = self.connect(&self.maintenance_db()).await?;
        drop_database_with_retry(&client, target).await?;

        let mut attempts = 0;
        loop {
            attempts += 1;
            // Identifier quoting is safe: both names were validated above.
            let sql = format!("CREATE DATABASE \"{target}\" TEMPLATE \"{template}\"");
            match client.batch_execute(&sql).await {
                Ok(()) => return Ok(()),
                Err(error) if attempts < 6 => {
                    tracing::warn!(
                        "CREATE DATABASE {target} from {template} failed (attempt {attempts}): {error}"
                    );
                    if attempts >= 2 {
                        // `mkdb.sh` does the same: a session attached to the
                        // template (ALLOW_CONNECTIONS false) blocks the clone.
                        let _ = terminate_connections(&client, template).await;
                    }
                    tokio::time::sleep(Duration::from_millis(500 * attempts)).await;
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("creating database {target:?} from template {template:?}")
                    });
                }
            }
        }
    }

    pub async fn drop_database(&self, name: &str) -> Result<()> {
        validate_identifier(name)?;
        let client = self.connect(&self.maintenance_db()).await?;
        drop_database_with_retry(&client, name).await
    }

    pub async fn create_database(&self, name: &str) -> Result<()> {
        validate_identifier(name)?;
        let client = self.connect(&self.maintenance_db()).await?;
        client
            .batch_execute(&format!("CREATE DATABASE \"{name}\""))
            .await
            .with_context(|| format!("creating database {name:?}"))?;
        Ok(())
    }

    pub async fn load_catalog(
        &self,
        dbname: &str,
        schemas: &[String],
        exclude: &[String],
    ) -> Result<Catalog> {
        let client = self.connect(dbname).await?;
        let schemas_vec = schemas.to_vec();
        let rows = client
            .query(
                "SELECT table_schema, table_name
                   FROM information_schema.tables
                  WHERE table_type = 'BASE TABLE'
                    AND table_schema = ANY($1::text[])
                  ORDER BY table_schema, table_name",
                &[&schemas_vec],
            )
            .await
            .context("listing tables")?;

        let pk_rows = client
            .query(
                "SELECT tc.table_schema, tc.table_name, kcu.column_name
                   FROM information_schema.table_constraints tc
                   JOIN information_schema.key_column_usage kcu
                     ON kcu.constraint_name = tc.constraint_name
                    AND kcu.constraint_schema = tc.constraint_schema
                    AND kcu.table_name = tc.table_name
                  WHERE tc.constraint_type = 'PRIMARY KEY'
                    AND tc.table_schema = ANY($1::text[])
                  ORDER BY tc.table_schema, tc.table_name, kcu.ordinal_position",
                &[&schemas_vec],
            )
            .await
            .context("listing primary keys")?;

        let mut pks: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
        for row in pk_rows {
            let schema: String = row.get(0);
            let table: String = row.get(1);
            let column: String = row.get(2);
            pks.entry((schema, table)).or_default().push(column);
        }

        let mut tables = Vec::new();
        for row in rows {
            let schema: String = row.get(0);
            let name: String = row.get(1);
            let qualified = format!("{schema}.{name}");
            if exclude.iter().any(|excluded| {
                excluded.eq_ignore_ascii_case(&qualified) || excluded.eq_ignore_ascii_case(&name)
            }) {
                continue;
            }
            let pk_columns = pks
                .remove(&(schema.clone(), name.clone()))
                .unwrap_or_default();
            tables.push(TableMeta {
                schema,
                name,
                pk_columns,
            });
        }
        Ok(Catalog { tables })
    }

    /// Dump every table in the catalog in a single round trip.
    pub async fn snapshot(&self, dbname: &str, catalog: &Catalog) -> Result<SnapshotResult> {
        let snapshotter = self.snapshotter(dbname, catalog.clone()).await?;
        snapshotter.snapshot().await
    }

    /// Open a dedicated connection for repeated snapshots of one database.
    /// Reusing it keeps the per-snapshot cost to the query itself.
    pub async fn snapshotter(&self, dbname: &str, catalog: Catalog) -> Result<DbSnapshotter> {
        let client = self.connect(dbname).await?;
        Ok(DbSnapshotter { client, catalog })
    }
}

pub struct DbSnapshotter {
    client: Client,
    catalog: Catalog,
}

impl DbSnapshotter {
    pub async fn snapshot(&self) -> Result<SnapshotResult> {
        let started = Instant::now();
        if self.catalog.tables.is_empty() {
            return Ok(SnapshotResult {
                snapshot: DbSnapshot::default(),
                duration: started.elapsed(),
            });
        }
        let sql = snapshot_query(&self.catalog);
        let messages = self
            .client
            .simple_query(&sql)
            .await
            .context("snapshot query")?;
        let snapshot = parse_snapshot(&self.catalog, messages)?;
        Ok(SnapshotResult {
            snapshot,
            duration: started.elapsed(),
        })
    }
}

fn snapshot_query(catalog: &Catalog) -> String {
    let mut sql = String::with_capacity(catalog.tables.len() * 96);
    for (index, table) in catalog.tables.iter().enumerate() {
        if index > 0 {
            sql.push_str(" UNION ALL ");
        }
        let quoted = format!("\"{}\".\"{}\"", table.schema, table.name);
        // The row label is the unquoted `schema.table`: it is what the
        // normalisation table and the report refer to.
        sql.push_str("SELECT '");
        sql.push_str(&format!("{}.{}", table.schema, table.name));
        sql.push_str("'::text AS tbl, to_jsonb(t)::text AS row FROM ");
        sql.push_str(&quoted);
        sql.push_str(" t");
    }
    sql
}

fn parse_snapshot(catalog: &Catalog, messages: Vec<SimpleQueryMessage>) -> Result<DbSnapshot> {
    let by_name: BTreeMap<String, &TableMeta> = catalog
        .tables
        .iter()
        .map(|meta| (format!("{}.{}", meta.schema, meta.name), meta))
        .collect();
    let mut snapshot = DbSnapshot::default();
    let mut duplicates: BTreeMap<String, usize> = BTreeMap::new();
    for message in messages {
        let SimpleQueryMessage::Row(row) = message else {
            continue;
        };
        let Some(table) = row.get(0) else { continue };
        let Some(text) = row.get(1) else { continue };
        let value: Value =
            serde_json::from_str(text).with_context(|| format!("parsing row JSON from {table}"))?;
        let meta = by_name
            .get(table)
            .ok_or_else(|| anyhow!("snapshot row for unknown table {table}"))?;
        let key = row_key(meta, &value, text)?;
        let key = if meta.pk_columns.is_empty() {
            let counter = duplicates.entry(key.clone()).or_insert(0);
            let unique = format!("{key}#{counter}");
            *counter += 1;
            unique
        } else {
            key
        };
        snapshot
            .tables
            .entry(table.to_string())
            .or_default()
            .insert(key, value);
    }
    Ok(snapshot)
}

fn row_key(meta: &TableMeta, row: &Value, raw: &str) -> Result<String> {
    if meta.pk_columns.is_empty() {
        return Ok(raw.to_string());
    }
    let map = row
        .as_object()
        .ok_or_else(|| anyhow!("row from {} is not a JSON object", meta.name))?;
    let mut values = Vec::with_capacity(meta.pk_columns.len());
    for column in &meta.pk_columns {
        match map.get(column) {
            Some(value) => values.push(value.clone()),
            None => bail!(
                "primary-key column {column:?} missing from a {} row",
                meta.name
            ),
        }
    }
    Ok(serde_json::to_string(&Value::Array(values))?)
}

/// `DROP DATABASE` fails while a session is still attached. A target that was
/// just killed can hold its connection for a moment, so terminate and retry.
async fn drop_database_with_retry(client: &Client, name: &str) -> Result<()> {
    let mut last_error: Option<String> = None;
    for attempt in 1..=5 {
        terminate_connections(client, name).await?;
        match client
            .batch_execute(&format!("DROP DATABASE IF EXISTS \"{name}\""))
            .await
        {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
                if attempt < 5 {
                    tokio::time::sleep(Duration::from_millis(200 * attempt)).await;
                }
            }
        }
    }
    Err(anyhow!(
        "dropping database {name:?}: {}",
        last_error.unwrap_or_else(|| "unknown error".to_string())
    ))
}

async fn terminate_connections(client: &Client, dbname: &str) -> Result<()> {
    let statement = "SELECT pg_terminate_backend(pid)
                       FROM pg_stat_activity
                      WHERE datname = $1
                        AND pid <> pg_backend_pid()";
    client
        .execute(statement, &[&dbname])
        .await
        .with_context(|| format!("terminating connections to {dbname:?}"))?;
    Ok(())
}

/// Compute inserted / deleted / updated rows per table.
pub fn diff_snapshots(before: &DbSnapshot, after: &DbSnapshot) -> DbDiff {
    let mut diff = DbDiff::default();
    let tables: BTreeSet<&String> = before.tables.keys().chain(after.tables.keys()).collect();
    for table in tables {
        let empty = BTreeMap::new();
        let before_rows = before.tables.get(table).unwrap_or(&empty);
        let after_rows = after.tables.get(table).unwrap_or(&empty);
        let mut table_diff = TableDiff::default();

        for (key, row) in after_rows {
            match before_rows.get(key) {
                None => table_diff.inserted.push(row.clone()),
                Some(previous) if previous != row => {
                    let changed = changed_columns(previous, row);
                    if !changed.is_empty() {
                        table_diff.updated.push(json!({
                            "pk": key_value(key),
                            "changed": changed,
                        }));
                    }
                }
                Some(_) => {}
            }
        }
        for (key, row) in before_rows {
            if !after_rows.contains_key(key) {
                table_diff.deleted.push(row.clone());
            }
        }

        if !table_diff.inserted.is_empty()
            || !table_diff.deleted.is_empty()
            || !table_diff.updated.is_empty()
        {
            diff.tables.insert(table.clone(), table_diff);
        }
    }
    diff
}

fn key_value(key: &str) -> Value {
    match serde_json::from_str::<Value>(key) {
        Ok(value @ Value::Array(_)) => value,
        _ => Value::String(key.to_string()),
    }
}

fn changed_columns(before: &Value, after: &Value) -> Map<String, Value> {
    let mut changed = Map::new();
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        if before != after {
            changed.insert("_row".to_string(), json!({"a": before, "b": after}));
        }
        return changed;
    };
    for (column, after_value) in after {
        match before.get(column) {
            Some(before_value) if before_value == after_value => {}
            Some(before_value) => {
                changed.insert(column.clone(), json!({"a": before_value, "b": after_value}));
            }
            None => {
                changed.insert(column.clone(), json!({"a": Value::Null, "b": after_value}));
            }
        }
    }
    for (column, before_value) in before {
        if !after.contains_key(column) {
            changed.insert(column.clone(), json!({"a": before_value, "b": Value::Null}));
        }
    }
    changed
}

/// A `difftest snapshot` style timing on an existing database.
pub async fn measure_snapshot(db: &DbAdmin, dbname: &str, iterations: usize) -> Result<()> {
    let catalog = db
        .load_catalog(dbname, &["public".to_string()], &[])
        .await?;
    println!(
        "database {dbname}: {} tables in schema public",
        catalog.table_count()
    );
    let snapshotter = db.snapshotter(dbname, catalog).await?;
    let mut timings = Vec::new();
    let mut last: Option<DbSnapshot> = None;
    for _ in 0..iterations.max(1) {
        let result = snapshotter.snapshot().await?;
        timings.push(result.duration);
        last = Some(result.snapshot);
    }
    let mut rows: Vec<(String, usize)> = last
        .as_ref()
        .map(|snapshot| {
            snapshot
                .tables
                .iter()
                .map(|(table, rows)| (table.clone(), rows.len()))
                .collect()
        })
        .unwrap_or_default();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total_rows: usize = rows.iter().map(|(_, count)| count).sum();
    println!("total rows: {total_rows}");
    println!("largest tables:");
    for (table, count) in rows.iter().take(15) {
        if *count > 0 {
            println!("  {table}: {count}");
        }
    }
    let min = timings.iter().min().copied().unwrap_or_default();
    let max = timings.iter().max().copied().unwrap_or_default();
    let avg = timings.iter().map(Duration::as_secs_f64).sum::<f64>() / timings.len() as f64;
    println!(
        "snapshot timing over {} run(s): min {:.1} ms, avg {:.1} ms, max {:.1} ms",
        timings.len(),
        min.as_secs_f64() * 1000.0,
        avg * 1000.0,
        max.as_secs_f64() * 1000.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(pk: &[&str]) -> TableMeta {
        TableMeta {
            schema: "public".to_string(),
            name: "t".to_string(),
            pk_columns: pk.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn diffs_insert_delete_update() {
        let mut before = DbSnapshot::default();
        let mut rows = BTreeMap::new();
        rows.insert(
            r#"["a"]"#.to_string(),
            json!({"id": "a", "name": "one", "n": 1}),
        );
        rows.insert(
            r#"["b"]"#.to_string(),
            json!({"id": "b", "name": "gone", "n": 2}),
        );
        before.tables.insert("public.t".to_string(), rows);

        let mut after = DbSnapshot::default();
        let mut rows = BTreeMap::new();
        rows.insert(
            r#"["a"]"#.to_string(),
            json!({"id": "a", "name": "two", "n": 1}),
        );
        rows.insert(
            r#"["c"]"#.to_string(),
            json!({"id": "c", "name": "new", "n": 3}),
        );
        after.tables.insert("public.t".to_string(), rows);

        let diff = diff_snapshots(&before, &after);
        let table = &diff.tables["public.t"];
        assert_eq!(table.inserted.len(), 1);
        assert_eq!(table.inserted[0]["id"], "c");
        assert_eq!(table.deleted.len(), 1);
        assert_eq!(table.deleted[0]["id"], "b");
        assert_eq!(table.updated.len(), 1);
        assert_eq!(table.updated[0]["pk"], json!(["a"]));
        let changed = table.updated[0]["changed"].as_object().expect("object");
        assert_eq!(changed.len(), 1);
        assert_eq!(changed["name"], json!({"a": "one", "b": "two"}));
    }

    #[test]
    fn row_keys_follow_the_catalog() {
        let value = json!({"id": "a", "x": 1});
        assert_eq!(
            row_key(&meta(&["id"]), &value, "").expect("key"),
            r#"["a"]"#
        );
        assert_eq!(
            row_key(&meta(&["id", "x"]), &value, "").expect("key"),
            r#"["a",1]"#
        );
        assert_eq!(row_key(&meta(&[]), &value, "raw").expect("key"), "raw");
        assert!(row_key(&meta(&["missing"]), &value, "").is_err());
    }

    #[test]
    fn empty_diff_is_reported_as_empty() {
        let snapshot = DbSnapshot::default();
        assert!(diff_snapshots(&snapshot, &snapshot).is_empty());
    }
}
