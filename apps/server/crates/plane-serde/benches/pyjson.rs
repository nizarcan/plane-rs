// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Parsing a realistic work-item update body, against `serde_json`.
//!
//! Run with:
//!
//! ```text
//! CARGO_TARGET_DIR=target/pyjson cargo bench -p plane-serde --bench pyjson
//! ```

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use plane_serde::pyjson;

/// A body the work-item update endpoint accepts: one object with a long HTML
/// description, labels, assignees, sub-issues and a few dozen scalar fields.
/// Around 30 KB, which is a large but realistic PATCH.
///
/// The description is spliced in with `{:?}` so its newlines become `\n`
/// escapes; a real client sends exactly that, and it makes the parser take
/// its escape-aware string path rather than the plain-run fast path.
fn sample_body() -> Vec<u8> {
    let description: String = (0..140)
        .map(|index| {
            format!(
                "<p>Step {index}: open the view, change the filter twice and scroll to the \
                 end. The cursor then points at the previous page — the second filter is \
                 dropped from the query string.</p>\n"
            )
        })
        .collect();
    let labels: Vec<String> = (0..24)
        .map(|index| format!("\"6f9c2c1e-4a44-4d3e-9b6f-6d8b0f9f3a{index:02x}\""))
        .collect();
    let assignees: Vec<String> = (0..8)
        .map(|index| format!("\"8a1b2c3d-4e5f-4a6b-8c9d-0e1f2a3b4c{index:02x}\""))
        .collect();
    let sub_issues: Vec<String> = (0..25)
        .map(|index| {
            format!(
                "{{\"id\":\"05000000-0000-0000-0000-0000000000{index:02x}\",\
                 \"name\":\"Sub-issue {index}\",\"sequence_id\":{sequence},\
                 \"priority\":\"medium\",\"state\":\"6f9c2c1e-4a44-4d3e-9b6f-6d8b0f9f3a11\",\
                 \"estimate\":{estimate}}}",
                sequence = 1000 + index,
                estimate = index as f64 / 2.0,
            )
        })
        .collect();
    let body = format!(
        r#"{{"name":"Fix flaky pagination cursor when filters change",
"description_html":{description:?},
"description_stripped":"When the filter is applied twice the cursor points at the previous page.",
"priority":"high","state":"6f9c2c1e-4a44-4d3e-9b6f-6d8b0f9f3a11",
"project":"07000000-0000-0000-0000-000000000000",
"assignees":[{assignees}],"labels":[{labels}],"sub_issues":[{sub_issues}],
"start_date":"2026-11-01","target_date":"2026-12-01",
"estimate":2.5,"points":3,"is_draft":false,"parent":null,
"meta":{{"created_by":"ada@example.com","ratio":0.125,"count":12,
"tags":["bug","pagination","backend","performance"],
"nested":{{"a":{{"b":{{"c":[1,2.5,"three",true,false,null]}}}}}}}}}}"#,
        description = description,
        assignees = assignees.join(","),
        labels = labels.join(","),
        sub_issues = sub_issues.join(","),
    );
    body.into_bytes()
}

fn benchmark(c: &mut Criterion) {
    let body = sample_body();
    if let Err(error) = pyjson::loads(&body, "utf-8") {
        panic!("body is valid JSON: {error:?}");
    }
    assert!(serde_json::from_slice::<serde_json::Value>(&body).is_ok());
    println!("issue-update body: {} bytes", body.len());

    let mut group = c.benchmark_group("parse_issue_update");
    group.throughput(Throughput::Bytes(body.len() as u64));
    group.bench_function("pyjson_loads", |bencher| {
        bencher.iter(|| {
            let value = pyjson::loads(black_box(&body), "utf-8").expect("parses");
            black_box(value.len())
        });
    });
    group.bench_function("serde_json_from_slice", |bencher| {
        bencher.iter(|| {
            let value: serde_json::Value =
                serde_json::from_slice(black_box(&body)).expect("parses");
            black_box(value.as_object().map(serde_json::Map::len))
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
