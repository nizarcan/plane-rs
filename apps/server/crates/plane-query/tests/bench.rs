// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Latency and round trips of a realistic grouped issue list.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_query_fixture
//! apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_query_fixture \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-query --test bench -- --ignored --nocapture
//! ```
//!
//! The Python half of the comparison is `apps/server/tools/query/bench.py`,
//! which times `GroupedOffsetPaginator.get_result` + `issue_on_results` +
//! `process_results` for the same request on the same database. The numbers are
//! recorded in `crates/plane-query/README.md`.

use std::time::Instant;

use chrono_tz::Tz;
use plane_drf::QueryDict;
use plane_query::issue::{ListRequest, run};
use uuid::Uuid;

fn database_url() -> Option<String> {
    std::env::var("PLANE_RS_TEST_DATABASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
}

struct Meta {
    workspace_slug: String,
    project_id: Uuid,
    fixed_now: chrono::DateTime<chrono::Utc>,
}

fn meta() -> Meta {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/meta.json");
    let text = std::fs::read_to_string(&path).expect("meta.json; run build_fixture.py");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse meta.json");
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

#[tokio::test]
#[ignore = "benchmark: needs the fixture database"]
async fn grouped_issue_list_latency() {
    let Some(url) = database_url() else {
        eprintln!("bench: PLANE_RS_TEST_DATABASE_URL is unset; skipping");
        return;
    };
    let meta = meta();
    let config: tokio_postgres::Config = url.parse().expect("database url");
    let manager = deadpool_postgres::Manager::from_config(
        config,
        tokio_postgres::NoTls,
        deadpool_postgres::ManagerConfig {
            recycling_method: deadpool_postgres::RecyclingMethod::Fast,
        },
    );
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(1)
        .runtime(deadpool_postgres::Runtime::Tokio1)
        .build()
        .expect("pool");
    let client = pool.get().await.expect("connect");
    let params = QueryDict::parse("group_by=state_id&per_page=100&order_by=-created_at", None)
        .expect("query");

    let request = |now| ListRequest {
        query: &params,
        workspace_slug: &meta.workspace_slug,
        project_id: meta.project_id,
        timezone: Tz::UTC,
        now,
        user_id: None,
        guest_view_all_features: true,
        is_guest: false,
    };

    // Warm-up: connection, plan cache, allocator.
    let body = run(&client, &request(meta.fixed_now))
        .await
        .expect("warm-up");
    let body = body.to_drf();

    const ITERATIONS: usize = 50;
    let before = plane_query::stats::statements();
    let mut samples: Vec<f64> = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let started = Instant::now();
        let response = run(&client, &request(meta.fixed_now)).await.expect("query");
        let rendered = response.to_drf();
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(rendered.len(), body.len(), "response size is stable");
    }
    let statements = plane_query::stats::statements() - before;

    samples.sort_by(|left, right| left.partial_cmp(right).expect("finite"));
    let p50 = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 19 / 20];
    println!(
        "rust: group_by=state_id per_page=100 response={} bytes",
        body.len()
    );
    println!(
        "rust: statements per page = {:.1}",
        statements as f64 / ITERATIONS as f64
    );
    println!("rust: p50 = {p50:.2} ms, p95 = {p95:.2} ms over {ITERATIONS} runs");
}
