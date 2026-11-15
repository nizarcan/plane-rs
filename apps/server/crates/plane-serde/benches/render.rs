// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Throughput of the DRF dialect against plain `serde_json` on a payload the
//! size of a typical work-item list page.
//!
//! Run with:
//!
//! ```text
//! CARGO_TARGET_DIR=target/serde cargo bench -p plane-serde --bench render
//! ```

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use plane_serde::Dialect;
use plane_serde::to_vec;
use plane_serde::values::{DrfDateTime, DrfDecimal, PyDate};
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
struct IssueList {
    total_count: i64,
    count: i64,
    next_cursor: String,
    prev_cursor: String,
    next_page_results: bool,
    prev_page_results: bool,
    issues: Vec<Issue>,
}

#[derive(Serialize)]
struct Issue {
    id: Uuid,
    sequence_id: i64,
    name: String,
    description_stripped: String,
    priority: &'static str,
    state: State,
    project: IdName,
    workspace: IdName,
    labels: Vec<Label>,
    assignees: Vec<Assignee>,
    created_at: DrfDateTime,
    updated_at: DrfDateTime,
    target_date: Option<PyDate>,
    start_date: Option<PyDate>,
    estimate: Option<f64>,
    points: Option<DrfDecimal>,
    sub_issues_count: i32,
    attachment_count: i32,
    is_draft: bool,
    archived_at: Option<DrfDateTime>,
}

#[derive(Serialize)]
struct State {
    id: Uuid,
    name: &'static str,
    group: &'static str,
    color: &'static str,
}

#[derive(Serialize)]
struct IdName {
    id: Uuid,
    name: &'static str,
}

#[derive(Serialize)]
struct Label {
    id: Uuid,
    name: &'static str,
    color: &'static str,
}

#[derive(Serialize)]
struct Assignee {
    id: Uuid,
    display_name: &'static str,
    avatar_url: Option<&'static str>,
    email: &'static str,
}

fn sample_payload() -> IssueList {
    let names = [
        "Fix flaky pagination cursor when filters change",
        "Add cycle rollover job",
        "Session cookie is set twice behind the proxy",
        "Expose estimate points in the work item list",
        "Search returns archived work items",
        "Notification email uses the wrong project name",
        "Webhook retries should back off exponentially",
        "Avatar upload rejects large PNG files",
    ];
    let labels = ["bug", "improvement", "backend", "frontend", "performance"];
    let people = [
        ("Ada Lovelace", "ada@example.com"),
        ("Grace Hopper", "grace@example.com"),
        ("Alan Turing", "alan@example.com"),
        ("Barbara Liskov", "barbara@example.com"),
    ];
    let state_names = ["Todo", "In Progress", "Done", "Backlog"];

    let base = 1_760_000_000_i64;
    let issues = (0..50_i64)
        .map(|index| Issue {
            id: Uuid::from_u128(0x0500_0000_0000_0000_0000_0000_0000_0000 + index as u128),
            sequence_id: 1000 + index,
            name: names[index as usize % names.len()].to_owned(),
            description_stripped: format!(
                "When the filter is applied twice the cursor points at the previous page. \
                 Steps to reproduce: open the view, change the filter, scroll to the end (#{}).",
                index
            ),
            priority: if index % 3 == 0 { "high" } else { "medium" },
            state: State {
                id: Uuid::from_u128(
                    0x0600_0000_0000_0000_0000_0000_0000_0000 + (index % 4) as u128,
                ),
                name: state_names[index as usize % state_names.len()],
                group: if index % 4 == 2 {
                    "completed"
                } else {
                    "started"
                },
                color: "#3f76ff",
            },
            project: IdName {
                id: Uuid::from_u128(0x0700_0000_0000_0000_0000_0000_0000_0000),
                name: "Apollo",
            },
            workspace: IdName {
                id: Uuid::from_u128(0x0800_0000_0000_0000_0000_0000_0000_0000),
                name: "acme",
            },
            labels: (0..(index % 3 + 1))
                .map(|offset| Label {
                    id: Uuid::from_u128(0x0900_0000_0000_0000_0000_0000_0000_0000 + offset as u128),
                    name: labels[(index + offset) as usize % labels.len()],
                    color: "#ff7700",
                })
                .collect(),
            assignees: (0..(index % 2 + 1))
                .map(|offset| {
                    let (display_name, email) = people[(index + offset) as usize % people.len()];
                    Assignee {
                        id: Uuid::from_u128(
                            0x0a00_0000_0000_0000_0000_0000_0000_0000 + offset as u128,
                        ),
                        display_name,
                        avatar_url: Some("https://example.com/avatar.png"),
                        email,
                    }
                })
                .collect(),
            created_at: DrfDateTime(
                chrono::DateTime::from_timestamp(base + index * 3600, 123_456_000).unwrap(),
            ),
            updated_at: DrfDateTime(
                chrono::DateTime::from_timestamp(base + index * 3600 + 60, 0).unwrap(),
            ),
            target_date: Some(PyDate(
                chrono::NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
            )),
            start_date: None,
            estimate: if index % 2 == 0 { Some(2.5) } else { None },
            points: Some(DrfDecimal::new("3.2500".parse().expect("decimal"), 12, 4)),
            sub_issues_count: (index % 5) as i32,
            attachment_count: (index % 2) as i32,
            is_draft: false,
            archived_at: None,
        })
        .collect();

    IssueList {
        total_count: 50,
        count: 50,
        next_cursor: "50:0:0".to_owned(),
        prev_cursor: "".to_owned(),
        next_page_results: false,
        prev_page_results: false,
        issues,
    }
}

fn benchmark(c: &mut Criterion) {
    let payload = sample_payload();
    let drf = to_vec(Dialect::Drf, &payload).expect("serialises");
    let plain = serde_json::to_vec(&payload).expect("serialises");
    assert!(!drf.is_empty());
    assert!(!plain.is_empty());

    println!(
        "issue-list payload: {} bytes (plane-serde) / {} bytes (serde_json)",
        drf.len(),
        plain.len()
    );

    let mut group = c.benchmark_group("issue_list");
    group.throughput(Throughput::Bytes(drf.len() as u64));
    group.bench_function("plane_serde_drf", |b| {
        b.iter(|| {
            let bytes = to_vec(Dialect::Drf, black_box(&payload)).expect("serialises");
            black_box(bytes.len())
        });
    });
    group.bench_function("serde_json", |b| {
        b.iter(|| {
            let bytes = serde_json::to_vec(black_box(&payload)).expect("serialises");
            black_box(bytes.len())
        });
    });
    group.finish();
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
