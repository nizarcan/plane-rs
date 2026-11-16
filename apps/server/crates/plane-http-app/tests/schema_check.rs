// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check` for every query catalog in this crate.
//!
//! Each domain owns `src/<domain>/queries.rs` with its own `ALL_QUERIES`; this
//! test is the one place that knows the complete list, so a domain agent adds
//! statements without touching a shared file. The `list_is_complete` test below
//! fails when a domain is missing here, so the list cannot silently fall behind
//! `src/lib.rs`.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_httpapp_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_httpapp_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-http-app --test schema_check
//! apps/server/dev/dropdb.sh t_httpapp_schema
//! ```
//!
//! The skip/require rules live in `plane_db::schema_check`, so every crate
//! behaves the same.

use plane_db::Query;
use plane_http_app::common::queries as common;

/// Every catalog this crate owns, in `src/lib.rs`'s registration order.
fn catalogs() -> Vec<(&'static str, &'static [&'static Query])> {
    vec![
        ("common", common::ALL_QUERIES),
        ("meta", plane_http_app::meta::queries::ALL_QUERIES),
        ("users_me", plane_http_app::users_me::queries::ALL_QUERIES),
        ("assets", plane_http_app::assets::queries::ALL_QUERIES),
        (
            "workspaces",
            plane_http_app::workspaces::queries::ALL_QUERIES,
        ),
        ("analytics", plane_http_app::analytics::queries::ALL_QUERIES),
        ("projects", plane_http_app::projects::queries::ALL_QUERIES),
        ("states", plane_http_app::states::queries::ALL_QUERIES),
        ("labels", plane_http_app::labels::queries::ALL_QUERIES),
        ("estimates", plane_http_app::estimates::queries::ALL_QUERIES),
        ("issues", plane_http_app::issues::queries::ALL_QUERIES),
        ("cycles", plane_http_app::cycles::queries::ALL_QUERIES),
        ("modules", plane_http_app::modules::queries::ALL_QUERIES),
        ("views", plane_http_app::views::queries::ALL_QUERIES),
        ("pages", plane_http_app::pages::queries::ALL_QUERIES),
        ("intake", plane_http_app::intake::queries::ALL_QUERIES),
        (
            "notifications",
            plane_http_app::notifications::queries::ALL_QUERIES,
        ),
        ("search", plane_http_app::search::queries::ALL_QUERIES),
        ("stickies", plane_http_app::stickies::queries::ALL_QUERIES),
        ("webhooks", plane_http_app::webhooks::queries::ALL_QUERIES),
        ("ai", plane_http_app::ai::queries::ALL_QUERIES),
    ]
}

#[test]
fn query_names_are_unique_across_the_crate() {
    // The catalog name is what `schema_check` prints and what a handler reads in
    // a stack trace, so two domains must not guess the same one.
    let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for (domain, queries) in catalogs() {
        for query in queries {
            if let Some(first) = seen.insert(query.name, domain) {
                panic!(
                    "query name {:?} is declared in both {first} and {domain}",
                    query.name
                );
            }
        }
    }
}

#[test]
fn list_is_complete() {
    // `src/lib.rs` declares one module per `/api/` route group; every one of
    // them has a `queries` module and has to be listed above. Catch a domain
    // that was added to the crate but not to this test.
    let source = include_str!("../src/lib.rs");
    let declared: Vec<&str> = source
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod "))
        .map(|name| name.trim_end_matches(';'))
        .filter(|name| !matches!(*name, "common"))
        .collect();
    let listed: Vec<&str> = catalogs()
        .into_iter()
        .map(|(domain, _)| domain)
        .filter(|domain| *domain != "common")
        .collect();
    for domain in &listed {
        assert!(
            declared.contains(domain),
            "{domain} is listed in tests/schema_check.rs but is not a module of src/lib.rs"
        );
    }
    for domain in &declared {
        assert!(
            listed.contains(domain),
            "src/lib.rs declares {domain} but tests/schema_check.rs does not check its catalog"
        );
    }
}

#[tokio::test]
async fn schema_check() {
    let url = match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "schema_check: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return;
        }
        Err(message) => panic!("{message}"),
    };

    let queries: Vec<&Query> = catalogs()
        .into_iter()
        .flat_map(|(_, queries)| queries.iter().copied())
        .collect();
    if queries.is_empty() {
        println!("schema_check: no statements catalogued yet");
        return;
    }
    match plane_db::schema_check::verify_url(&url, &queries).await {
        Ok(report) => println!(
            "schema_check: {} statements verified against {}",
            report.checked, report.target
        ),
        Err(error) => panic!("{error}"),
    }
}
