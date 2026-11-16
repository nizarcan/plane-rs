// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Replay the Django-generated goldens against a database built by
//! `tools/query/build_fixture.py`.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_query_fixture
//! apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
//! PLANE_DB=t_query_fixture apps/server/tools/query/run.sh \
//!   apps/server/tools/query/oracle.py
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_query_fixture \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-query --test golden
//! ```
//!
//! Every test compares against the golden file written by `oracle.py`; nothing
//! is regenerated here and no expectation is relaxed. The one documented
//! normalisation is the order of the id arrays
//! `GroupedOffsetPaginator.__query_multi_grouper` builds from a Python `set`
//! (see `normalise_group_arrays` in the oracle); the golden already stores them
//! sorted, and the port sorts them too.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_drf::QueryDict;
use plane_query::error::QueryError;
use plane_query::global;
use plane_query::issue::filter::{FilterParams, ParamValue};
use plane_query::issue::model::{GroupField, ValueField};
use plane_query::issue::order::Ordering;
use plane_query::issue::query::{IssueQuery, IssueScope};
use plane_query::issue::rich;
use plane_query::issue::{group_values, values};
use plane_query::json::Json;
use plane_query::paginator::PageQuery;
use plane_query::sql::{Frag, Select};
use uuid::Uuid;

const TEST_DATABASE_URL: &str = "PLANE_RS_TEST_DATABASE_URL";
const REQUIRE_DB_TESTS: &str = "PLANE_RS_REQUIRE_DB_TESTS";

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn golden(name: &str) -> serde_json::Value {
    let path = golden_dir().join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

struct Meta {
    workspace_slug: String,
    project_id: Uuid,
    fixed_now: DateTime<Utc>,
}

fn meta() -> Meta {
    let value = golden("meta.json");
    Meta {
        workspace_slug: value["workspace_slug"].as_str().expect("slug").to_owned(),
        project_id: value["project_id"]
            .as_str()
            .expect("project")
            .parse()
            .expect("uuid"),
        fixed_now: value["fixed_now"]
            .as_str()
            .expect("fixed_now")
            .parse()
            .expect("datetime"),
    }
}

/// Connect, or skip when no database is configured (CI sets
/// `PLANE_RS_REQUIRE_DB_TESTS=1` so a skip is impossible).
async fn connect() -> Option<Client> {
    let url = match std::env::var(TEST_DATABASE_URL)
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(url) => url,
        None => {
            if std::env::var(REQUIRE_DB_TESTS).as_deref() == Ok("1") {
                panic!("{REQUIRE_DB_TESTS}=1 but {TEST_DATABASE_URL} is unset");
            }
            eprintln!(
                "golden: {TEST_DATABASE_URL} is unset; skipping (build the fixture with \
                 tools/query/build_fixture.py)"
            );
            return None;
        }
    };
    let config: tokio_postgres::Config = url.parse().expect("database url");
    let manager = deadpool_postgres::Manager::from_config(
        config,
        tokio_postgres::NoTls,
        deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        },
    );
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(2)
        .runtime(deadpool_postgres::Runtime::Tokio1)
        .build()
        .expect("pool");
    let client = pool.get().await.expect("connect");
    Some(client)
}

async fn run_select(
    client: &Client,
    select: &Select,
) -> Result<Vec<tokio_postgres::Row>, QueryError> {
    let sql = select.clone().into_sql();
    if std::env::var("PLANE_QUERY_DEBUG_SQL").as_deref() == Ok("1") {
        eprintln!("SQL: {}\nparams: {:?}", sql.text, sql.params);
    }
    let statement = client.prepare(&sql.text).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    Ok(client.query(&statement, &params).await?)
}

async fn run_count(client: &Client, select: &Select, distinct: bool) -> Result<i64, QueryError> {
    let sql = select
        .clone()
        .count_sql(if distinct { Some("issues.id") } else { None }, None);
    let statement = client.prepare(&sql.text).await?;
    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
        .params
        .iter()
        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect();
    let row = client.query_one(&statement, &params).await?;
    Ok(row.get(0))
}

fn parse_ids(value: &serde_json::Value) -> Vec<String> {
    value
        .as_str()
        .unwrap_or_default()
        .split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn ids_from_rows(rows: &[tokio_postgres::Row], index: usize) -> Vec<String> {
    let mut ids: Vec<String> = ids_in_order(rows, index);
    ids.sort();
    ids
}

fn ids_in_order(rows: &[tokio_postgres::Row], index: usize) -> Vec<String> {
    rows.iter()
        .map(|row| row.get::<_, Uuid>(index).to_string())
        .collect()
}

fn base_query(meta: &Meta, filters: plane_query::issue::filter::LegacyFilters) -> IssueQuery {
    IssueQuery {
        scope: IssueScope::project(meta.workspace_slug.clone(), meta.project_id),
        filters,
        rich: None,
        ordering: Ordering::resolve("-created_at"),
        group_by: None,
        sub_group_by: None,
        timezone: Tz::UTC,
    }
}

fn ids_select(query: &IssueQuery, distinct: bool) -> Select {
    let mut select = query.filtered_base();
    select.distinct = distinct;
    select.projection = vec![Frag::raw("issues.id")];
    select.order_by = vec![Frag::raw("issues.id ASC")];
    select.group_by.clear();
    select
}

#[tokio::test]
async fn filters_match_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("filters.json");
    let cases = data["cases"].as_array().expect("cases");
    let now = meta.fixed_now;
    let mut matched = 0usize;
    let mut expected_errors = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case in cases {
        let id = case["id"].as_i64().unwrap_or_default();
        if let Ok(only) = std::env::var("PLANE_QUERY_CASE")
            && id.to_string() != only
        {
            continue;
        }
        let method = case["method"].as_str().unwrap_or("GET");
        let params_value = &case["params"];
        let filter_params = if method == "GET" {
            let text = params_value.as_str().unwrap_or_default();
            let dict: &'static QueryDict =
                Box::leak(Box::new(QueryDict::parse(text, None).expect("query")));
            FilterParams::Get(dict)
        } else {
            let owned: &'static Vec<(String, ParamValue)> =
                Box::leak(Box::new(post_params(params_value)));
            FilterParams::Post(
                owned
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.clone()))
                    .collect(),
            )
        };
        let parsed =
            plane_query::issue::filter::LegacyFilters::parse(&filter_params, now.date_naive());
        match (parsed, case.get("error")) {
            (Err(_), Some(_)) => {
                expected_errors += 1;
                continue;
            }
            (Err(error), None) => {
                failures.push(format!(
                    "case {id} {}: unexpected error {error}",
                    describe(params_value)
                ));
                continue;
            }
            (Ok(_), Some(error)) => {
                failures.push(format!(
                    "case {id} {}: Django answered {} but the port succeeded",
                    describe(params_value),
                    error.as_str().unwrap_or("an error")
                ));
                continue;
            }
            (Ok(filters), None) => {
                let query = base_query(&meta, filters);
                let (raw, distinct, rows) = match (
                    run_count(&client, &ids_select(&query, false), false).await,
                    run_count(&client, &ids_select(&query, true), true).await,
                    run_select(&client, &ids_select(&query, false)).await,
                ) {
                    (Ok(raw), Ok(distinct), Ok(rows)) => (raw, distinct, rows),
                    (raw, distinct, rows) => {
                        let error = raw
                            .err()
                            .or_else(|| distinct.err())
                            .or_else(|| rows.err())
                            .map(|error| error.to_string())
                            .unwrap_or_default();
                        failures.push(format!("case {id} {}: {error}", describe(params_value)));
                        continue;
                    }
                };
                let expected_raw = case["raw_count"].as_i64().unwrap_or_default();
                let expected_distinct = case["distinct_count"].as_i64().unwrap_or_default();
                let expected_ids = parse_ids(&case["ids"]);
                let actual_ids = ids_from_rows(&rows, 0);
                if raw != expected_raw {
                    failures.push(format!(
                        "case {id} {}: raw_count {raw} != {expected_raw}",
                        describe(params_value)
                    ));
                }
                if distinct != expected_distinct {
                    failures.push(format!(
                        "case {id} {}: distinct_count {distinct} != {expected_distinct}",
                        describe(params_value)
                    ));
                }
                if actual_ids != expected_ids {
                    failures.push(format!(
                        "case {id} {}: {} ids != {} (first difference at {})",
                        describe(params_value),
                        actual_ids.len(),
                        expected_ids.len(),
                        first_difference(&actual_ids, &expected_ids)
                    ));
                }
                matched += 1;
            }
        }
    }
    report("filters", cases.len(), matched, expected_errors, &failures);
}

fn post_params(value: &serde_json::Value) -> Vec<(String, ParamValue)> {
    let object = value.as_object().expect("POST params object");
    object
        .iter()
        .map(|(key, value)| (key.clone(), param_value(value)))
        .collect()
}

fn param_value(value: &serde_json::Value) -> ParamValue {
    match value {
        serde_json::Value::Null => ParamValue::Null,
        serde_json::Value::Bool(flag) => ParamValue::Bool(*flag),
        serde_json::Value::Number(number) => number.as_i64().map_or_else(
            || ParamValue::Float(number.as_f64().unwrap_or_default()),
            ParamValue::Int,
        ),
        serde_json::Value::String(text) => ParamValue::Text(text.clone()),
        serde_json::Value::Array(items) => {
            ParamValue::List(items.iter().map(param_value).collect())
        }
        other => ParamValue::Text(other.to_string()),
    }
}

fn describe(params: &serde_json::Value) -> String {
    let text = params.to_string();
    if text.len() > 90 {
        format!("{}…", &text[..90])
    } else {
        text
    }
}

fn first_difference(actual: &[String], expected: &[String]) -> usize {
    actual
        .iter()
        .zip(expected.iter())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| actual.len().min(expected.len()))
}

fn report(category: &str, total: usize, matched: usize, errors: usize, failures: &[String]) {
    if !failures.is_empty() {
        for failure in failures.iter().take(25) {
            eprintln!("{category}: {failure}");
        }
        panic!(
            "{category}: {} of {total} cases failed ({} matched, {} expected-error cases)",
            failures.len(),
            matched,
            errors
        );
    }
    println!("golden {category}: {matched} matched, {errors} expected errors, {total} cases");
}

#[tokio::test]
async fn ordering_matches_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("order.json");
    let cases = data["cases"].as_array().expect("cases");
    let mut matched = 0usize;
    let mut failures = Vec::new();

    for case in cases {
        let order_by = case["order_by"].as_str().unwrap_or_default();
        let labels_filter = case.get("filter").is_some();
        let mut filters = plane_query::issue::filter::LegacyFilters::default();
        if labels_filter {
            filters.labels = Some(vec![
                Uuid::parse_str("7132efef-4997-5ef8-b97d-5d75d62542e8").expect("alpha"),
                Uuid::parse_str("43b4e751-4243-56ef-a3ee-e2d1316c5a09").expect("beta"),
            ]);
            filters.labels_join = true;
        }
        let query = IssueQuery {
            ordering: Ordering::resolve(order_by),
            filters,
            ..base_query(&meta, Default::default())
        };
        let returned = query.ordering.returned.clone();
        // Only the primary key and the ordering expression, like Django's
        // `values_list("id")` plus its ordering columns.
        let fields: Vec<ValueField> = Vec::new();
        let mut own = query.filtered_base();
        own.distinct = true;
        own.projection = vec![
            Frag::raw("issues.id"),
            query.ordering.expression.clone(),
            Frag::raw("issues.created_at"),
        ];
        query.ordering.apply_own_order(&mut own, &fields);
        if query.ordering.group_by_issue {
            own.group_by = vec!["issues.id".to_owned()];
        }
        let rows = match run_select(&client, &own).await {
            Ok(rows) => rows,
            Err(error) => {
                failures.push(format!("order_by {order_by:?}: {error}"));
                continue;
            }
        };
        let expected = parse_ids(&case["ids"]);

        let mut paginated = query.filtered_base();
        paginated.distinct = true;
        paginated.projection = vec![
            Frag::raw("issues.id"),
            query.ordering.expression.clone(),
            Frag::raw("issues.created_at"),
        ];
        query
            .ordering
            .apply_paginator_order(&mut paginated, &fields);
        if query.ordering.group_by_issue {
            paginated.group_by = vec!["issues.id".to_owned()];
        }
        let paginated_rows = if case.get("paginated_ids").is_some() {
            match run_select(&client, &paginated).await {
                Ok(rows) => rows,
                Err(error) => {
                    failures.push(format!("order_by {order_by:?}: {error}"));
                    continue;
                }
            }
        } else {
            Vec::new()
        };
        let expected_paginated = case.get("paginated_ids").map(parse_ids).unwrap_or_default();

        let actual = ids_in_order(&rows, 0);
        let actual_paginated = ids_in_order(&paginated_rows, 0);
        if Some(returned.as_str()) != case["returned"].as_str() {
            failures.push(format!(
                "order_by {order_by:?}: returned {returned:?} != {:?}",
                case["returned"]
            ));
        }
        if actual != expected {
            if std::env::var("PLANE_QUERY_DEBUG").as_deref() == Ok("1") {
                eprintln!(
                    "DEBUG order {order_by:?} own actual {:?} expected {:?}",
                    &actual[..actual.len().min(4)],
                    &expected[..expected.len().min(4)]
                );
            }
            failures.push(format!(
                "order_by {order_by:?}: ids differ at {}",
                first_difference(&actual, &expected)
            ));
        }
        if case.get("paginated_ids").is_some() && actual_paginated != expected_paginated {
            if std::env::var("PLANE_QUERY_DEBUG").as_deref() == Ok("1") {
                eprintln!(
                    "DEBUG order {order_by:?} paginated actual {:?} expected {:?}",
                    &actual_paginated[..actual_paginated.len().min(4)],
                    &expected_paginated[..expected_paginated.len().min(4)]
                );
            }
            failures.push(format!(
                "order_by {order_by:?}: paginated ids differ at {}",
                first_difference(&actual_paginated, &expected_paginated)
            ));
        }
        matched += 1;
    }
    report("order", cases.len(), matched, 0, &failures);
}

#[tokio::test]
async fn group_values_match_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("group_values.json");
    let mut failures = Vec::new();
    let mut matched = 0usize;
    let mut total = 0usize;

    for scope in ["project", "workspace"] {
        let Some(expected) = data[scope].as_object() else {
            continue;
        };
        for (field_name, expected_values) in expected {
            total += 1;
            let Some(field) = GroupField::parse(field_name) else {
                failures.push(format!("{scope}: unknown group field {field_name}"));
                continue;
            };
            let query = base_query(&meta, Default::default());
            let filtered = query.filtered_base();
            let request = group_values::GroupValuesRequest {
                field,
                workspace_slug: &meta.workspace_slug,
                project_id: if scope == "project" {
                    Some(meta.project_id)
                } else {
                    None
                },
                user: if scope == "project" {
                    None
                } else {
                    Some(uuid_of(&meta, "alice"))
                },
                issues: Some(&filtered),
            };
            let plan = match group_values::values(&request) {
                Ok(plan) => plan,
                Err(error) => {
                    failures.push(format!("{scope}/{field_name}: {error}"));
                    continue;
                }
            };
            let actual = match plan {
                group_values::GroupValues::Static(values) => values,
                group_values::GroupValues::Query { sql, append_none } => {
                    let statement = client.prepare(&sql.text).await.expect("prepare");
                    let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> = sql
                        .params
                        .iter()
                        .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync))
                        .collect();
                    let rows = client.query(&statement, &params).await.expect("query");
                    let mut values: Vec<String> = rows
                        .iter()
                        .map(|row| {
                            plane_query::issue::query::render_group_value(row, field)
                                .expect("render")
                        })
                        .collect();
                    if append_none {
                        values.push("None".to_owned());
                    }
                    values
                }
            };
            let expected: Vec<String> = expected_values
                .as_array()
                .expect("array")
                .iter()
                .map(|value| value.as_str().unwrap_or_default().to_owned())
                .collect();
            if actual != expected {
                failures.push(format!(
                    "{scope}/{field_name}: {} values != {} (first difference at {})",
                    actual.len(),
                    expected.len(),
                    first_difference(&actual, &expected)
                ));
            }
            matched += 1;
        }
    }
    report("group values", total, matched, 0, &failures);
}

fn uuid_of(meta: &Meta, user: &str) -> Uuid {
    // The fixture meta carries the user ids; this helper reads them lazily.
    let value = golden("meta.json");
    let _ = meta;
    value["users"][user]["id"]
        .as_str()
        .expect("user id")
        .parse()
        .expect("uuid")
}

#[tokio::test]
async fn paginator_matches_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("paginator.json");
    let mut cases = match &data {
        serde_json::Value::Array(items) => {
            items[0]["cases"].as_array().cloned().unwrap_or_default()
        }
        value => value["cases"].as_array().cloned().unwrap_or_default(),
    };
    if cases.is_empty() {
        cases = data["cases"].as_array().cloned().unwrap_or_default();
    }
    let errors = match &data {
        serde_json::Value::Array(items) => {
            items[0]["errors"].as_array().cloned().unwrap_or_default()
        }
        value => value["errors"].as_array().cloned().unwrap_or_default(),
    };

    let mut matched = 0usize;
    let mut pages = 0usize;
    let mut expected_errors = 0usize;
    let mut failures = Vec::new();

    for case in &cases {
        let group_by = case["group_by"].as_str();
        let sub_group_by = case["sub_group_by"].as_str();
        let order_by = case["order_by"].as_str().unwrap_or("-created_at");
        let per_page = case["per_page"].as_i64().unwrap_or(1000);
        let filter_params = case["filter_params"].as_str().unwrap_or_default();
        let mut base_params = format!("per_page={per_page}&order_by={order_by}");
        if let Some(group) = group_by {
            base_params.push_str(&format!("&group_by={group}"));
        }
        if let Some(sub) = sub_group_by {
            base_params.push_str(&format!("&sub_group_by={sub}"));
        }
        if !filter_params.is_empty() {
            base_params.push('&');
            base_params.push_str(filter_params);
        }

        if let Some(kind) = case.get("error") {
            let params = QueryDict::parse(&base_params, None).expect("query");
            let result = run_list(&client, &meta, &params).await;
            match result {
                Err(_) => expected_errors += 1,
                Ok(_) => failures.push(format!(
                    "case {} ({} <- {}): Django raised {} but the port returned a body",
                    case["id"],
                    group_by.unwrap_or("none"),
                    sub_group_by.unwrap_or("none"),
                    kind.as_str().unwrap_or("an error")
                )),
            }
            continue;
        }

        let mut case_failed = false;
        for page in case["pages"].as_array().cloned().unwrap_or_default() {
            let cursor = page["cursor"].as_str().unwrap_or_default();
            let mut params = base_params.clone();
            params.push_str(&format!("&cursor={cursor}"));
            let expected = page["body"].as_str().expect("body");
            match run_list(
                &client,
                &meta,
                &QueryDict::parse(&params, None).expect("query"),
            )
            .await
            {
                Ok(body) => {
                    let actual = body.to_drf();
                    pages += 1;
                    if actual != expected.as_bytes() {
                        case_failed = true;
                        failures.push(format!(
                            "case {} ({} <- {}) cursor {cursor}: body differs ({} vs {} bytes); {}",
                            case["id"],
                            group_by.unwrap_or("none"),
                            sub_group_by.unwrap_or("none"),
                            actual.len(),
                            expected.len(),
                            body_difference(&actual, expected.as_bytes())
                        ));
                    }
                }
                Err(error) => {
                    case_failed = true;
                    failures.push(format!(
                        "case {} cursor {cursor}: unexpected error {error}",
                        case["id"]
                    ));
                }
            }
        }
        for page in case["backward"].as_array().cloned().unwrap_or_default() {
            let cursor = page["cursor"].as_str().unwrap_or_default();
            let mut params = base_params.clone();
            params.push_str(&format!("&cursor={cursor}"));
            let expected = page["body"].as_str().expect("body");
            match run_list(
                &client,
                &meta,
                &QueryDict::parse(&params, None).expect("query"),
            )
            .await
            {
                Ok(body) => {
                    let actual = body.to_drf();
                    pages += 1;
                    if actual != expected.as_bytes() {
                        case_failed = true;
                        failures.push(format!(
                            "case {} cursor {cursor} (backward): body differs; {}",
                            case["id"],
                            body_difference(&actual, expected.as_bytes())
                        ));
                    }
                }
                Err(error) => {
                    case_failed = true;
                    failures.push(format!(
                        "case {} cursor {cursor} (backward): unexpected error {error}",
                        case["id"]
                    ));
                }
            }
        }
        if !case_failed {
            matched += 1;
        }
    }

    // Request-level rejections.
    for case in &errors {
        let params = case["params"].as_str().unwrap_or_default();
        let result = run_list(
            &client,
            &meta,
            &QueryDict::parse(params, None).expect("query"),
        )
        .await;
        match (result, case["status"].as_i64()) {
            (Err(QueryError::Api(error)), Some(status)) => {
                if i64::from(error.status) != status {
                    failures.push(format!("params {params:?}: status != {status}"));
                } else {
                    expected_errors += 1;
                }
            }
            (Err(QueryError::Unsupported(message)), None) => {
                let _ = message;
                expected_errors += 1;
            }
            (Err(error), _) => failures.push(format!("params {params:?}: {error}")),
            (Ok(_), _) => failures.push(format!("params {params:?}: expected a 400")),
        }
    }

    println!(
        "golden paginator: {matched} of {} cases matched across {pages} pages, {expected_errors} request-level errors",
        cases.len()
    );
    if !failures.is_empty() {
        for failure in failures.iter().take(25) {
            eprintln!("paginator: {failure}");
        }
        panic!(
            "paginator: {} failures across {} cases ({pages} pages)",
            failures.len(),
            cases.len()
        );
    }
}

async fn run_list(client: &Client, meta: &Meta, params: &QueryDict) -> Result<Json, QueryError> {
    plane_query::issue::run(
        client,
        &plane_query::issue::ListRequest {
            query: params,
            workspace_slug: &meta.workspace_slug,
            project_id: meta.project_id,
            timezone: Tz::UTC,
            now: meta.fixed_now,
            user_id: None,
            guest_view_all_features: true,
            is_guest: false,
        },
    )
    .await
}

fn body_difference(actual: &[u8], expected: &[u8]) -> String {
    let length = actual.len().min(expected.len());
    let index = (0..length).find(|index| actual[*index] != expected[*index]);
    match index {
        Some(index) => {
            let start = index.saturating_sub(60);
            let end = (index + 120).min(length);
            format!(
                "first difference at byte {index}: actual {:?} expected {:?}",
                String::from_utf8_lossy(&actual[start..end.min(actual.len())]),
                String::from_utf8_lossy(&expected[start..end.min(expected.len())])
            )
        }
        None => format!(
            "one is a prefix of the other (actual {} bytes, expected {} bytes)",
            actual.len(),
            expected.len()
        ),
    }
}

#[tokio::test]
async fn filterset_matches_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("filterset.json");
    let mut matched = 0usize;
    let mut total = 0usize;
    let mut failures = Vec::new();

    let spec_names: Vec<String> = data["spec"]
        .as_object()
        .expect("spec")
        .keys()
        .cloned()
        .collect();
    let mut ported = rich::filter_names();
    ported.sort();
    let mut expected = spec_names;
    expected.sort();
    if ported != expected {
        failures.push(format!(
            "filterset declaration list differs: port has {} entries, Python has {}",
            ported.len(),
            expected.len()
        ));
    }

    for case in data["queryset_cases"]
        .as_array()
        .cloned()
        .unwrap_or_default()
    {
        total += 1;
        let params = case["params"].as_str().unwrap_or_default();
        let query = QueryDict::parse(params, None).expect("query");
        let parsed = rich::parse_filterset(&query, Tz::UTC);
        match (parsed, case["status"].as_i64()) {
            (Err(QueryError::Api(error)), Some(400)) => {
                let actual = error.detail().to_serde();
                let expected = case["errors"].clone();
                if actual != expected {
                    failures.push(format!(
                        "filterset {params:?}: errors {actual} != {expected}"
                    ));
                } else {
                    matched += 1;
                }
            }
            (Err(error), _) => failures.push(format!("filterset {params:?}: {error}")),
            (Ok(_rich), Some(400)) => failures.push(format!(
                "filterset {params:?}: Django rejected it ({}), the port accepted it",
                case["errors"]
            )),
            (Ok(parsed), _) => {
                let mut query = base_query(&meta, Default::default());
                query.rich = parsed;
                query.scope.distinct = false;
                let rows = match run_select(&client, &ids_select(&query, false)).await {
                    Ok(rows) => rows,
                    Err(error) => {
                        failures.push(format!("filterset {params:?}: {error}"));
                        continue;
                    }
                };
                let actual = ids_from_rows(&rows, 0);
                let expected = parse_ids(&case["ids"]);
                if actual != expected {
                    failures.push(format!(
                        "filterset {params:?}: ids differ at {}",
                        first_difference(&actual, &expected)
                    ));
                } else {
                    matched += 1;
                }
            }
        }
    }

    for case in data["json_cases"].as_array().cloned().unwrap_or_default() {
        total += 1;
        let filter = case["filter"].clone();
        let parsed = if let Some(text) = filter.as_str() {
            rich::parse_complex_query(
                &QueryDict::parse(&format!("filters={text}"), None).expect("query"),
                Tz::UTC,
            )
        } else {
            rich::parse_complex_json(&filter, Tz::UTC)
        };
        if let Some(expected_error) = case.get("error") {
            match parsed {
                Err(QueryError::Api(error)) => {
                    let actual = error.detail().to_serde();
                    if actual != *expected_error {
                        failures.push(format!("json {filter}: error {actual} != {expected_error}"));
                    } else {
                        matched += 1;
                    }
                }
                Err(error) => failures.push(format!("json {filter}: {error}")),
                Ok(_) => failures.push(format!("json {filter}: expected an error")),
            }
            continue;
        }
        match parsed {
            Ok(parsed) => {
                let mut query = base_query(&meta, Default::default());
                query.rich = parsed;
                query.scope.distinct = false;
                let rows = match run_select(&client, &ids_select(&query, false)).await {
                    Ok(rows) => rows,
                    Err(error) => {
                        failures.push(format!("json {filter}: {error}"));
                        continue;
                    }
                };
                let actual = ids_from_rows(&rows, 0);
                let expected = parse_ids(&case["ids"]);
                if actual != expected {
                    if std::env::var("PLANE_QUERY_DEBUG").as_deref() == Ok("1") {
                        eprintln!(
                            "DEBUG json {filter}: actual {} {:?} expected {} {:?}",
                            actual.len(),
                            &actual[..actual.len().min(3)],
                            expected.len(),
                            &expected[..expected.len().min(3)]
                        );
                    }
                    failures.push(format!(
                        "json {filter}: ids differ at {}",
                        first_difference(&actual, &expected)
                    ));
                } else {
                    matched += 1;
                }
            }
            Err(error) => failures.push(format!("json {filter}: {error}")),
        }
    }

    report("filterset", total, matched, 0, &failures);
}

#[tokio::test]
async fn global_paginator_matches_django() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let data = golden("global_paginator.json");
    let mut matched = 0usize;
    let mut failures = Vec::new();

    for case in data["cases"].as_array().cloned().unwrap_or_default() {
        let cursor = case["cursor"].as_str();
        let timezone: Tz = case["timezone"]
            .as_str()
            .unwrap_or("UTC")
            .parse()
            .expect("tz");
        let fields = global_fields();
        let mut query = base_query(&meta, Default::default());
        query.scope.distinct = false;
        let result = global::page(&client, &query, &fields, cursor, timezone).await;
        let expected = case["body"].as_str().expect("body");
        match result {
            Ok(body) => {
                let actual = body.to_drf();
                if actual != expected.as_bytes() {
                    failures.push(format!(
                        "global cursor {cursor:?} ({timezone}): {}",
                        body_difference(&actual, expected.as_bytes())
                    ));
                } else {
                    matched += 1;
                }
            }
            Err(error) => failures.push(format!("global cursor {cursor:?}: {error}")),
        }
    }
    for case in data["errors"].as_array().cloned().unwrap_or_default() {
        let cursor = case["cursor"].as_str();
        let mut query = base_query(&meta, Default::default());
        query.scope.distinct = false;
        let result = global::page(&client, &query, &global_fields(), cursor, Tz::UTC).await;
        match (result, case.get("error")) {
            (Err(_), Some(_)) => matched += 1,
            (Err(error), None) => failures.push(format!("global cursor {cursor:?}: {error}")),
            (Ok(_), _) => failures.push(format!(
                "global cursor {cursor:?}: Django raised {} but the port returned a body",
                case["error"]
            )),
        }
    }
    report("global paginator", matched, matched, 0, &failures);
}

fn global_fields() -> Vec<ValueField> {
    vec![
        ValueField::Id,
        ValueField::Name,
        ValueField::StateId,
        ValueField::StateGroup,
        ValueField::SortOrder,
        ValueField::CompletedAt,
        ValueField::EstimatePoint,
        ValueField::Priority,
        ValueField::StartDate,
        ValueField::TargetDate,
        ValueField::SequenceId,
        ValueField::ProjectId,
        ValueField::ParentId,
        ValueField::CycleId,
        ValueField::CreatedAt,
        ValueField::UpdatedAt,
        ValueField::CreatedBy,
        ValueField::UpdatedBy,
        ValueField::IsDraft,
        ValueField::ArchivedAt,
        ValueField::ModuleIds,
        ValueField::LabelIds,
        ValueField::AssigneeIds,
        ValueField::LinkCount,
        ValueField::AttachmentCount,
        ValueField::SubIssuesCount,
    ]
}

/// A tiny sanity test that the fixture the goldens were built on is the one the
/// database holds: 225 visible issues in the main project.
#[tokio::test]
async fn fixture_is_the_expected_one() {
    let Some(client) = connect().await else {
        return;
    };
    let meta = meta();
    let query = base_query(&meta, Default::default());
    let count = run_count(&client, &ids_select(&query, true), true)
        .await
        .expect("count");
    assert_eq!(
        count, 225,
        "fixture: expected 225 visible issues in the main project"
    );
    let date: NaiveDate = meta.fixed_now.date_naive();
    assert_eq!(date.to_string(), "2026-06-15");
}

/// The value fields are compared by `ValueField` in the tests; this keeps the
/// import used when a future case needs the array variant.
#[allow(dead_code)]
fn grouped_fields(group: GroupField, sub: Option<GroupField>) -> Vec<ValueField> {
    ValueField::project(Some(group), sub)
}

/// A `BTreeMap` of the group totals, for tests that want to inspect them.
#[allow(dead_code)]
fn totals_map(pairs: &[(String, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().cloned().collect()
}

/// `PageQuery` is exercised through `issue::run`; this keeps the import alive
/// for the helper that builds a page directly in future tests.
#[allow(dead_code)]
fn page_query(params: &str) -> PageQuery {
    PageQuery::parse(&QueryDict::parse(params, None).expect("query"), None, None).expect("page")
}

/// `values::projection` is used through `issue::run`; keep the import.
#[allow(dead_code)]
fn projection_for(fields: &[ValueField]) -> Vec<Frag> {
    values::projection(fields)
}
