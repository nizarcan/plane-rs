// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check` for the v1 catalogs in this crate.
//!
//! Mirrors `crates/plane-http-app/tests/schema_check.rs`: every `*queries.rs`
//! file under `src/` must be listed here, and every statement is prepared
//! against a Django-migrated database so a column name or type drift fails.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_v1d_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_v1d_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-http-public --test schema_check
//! apps/server/dev/dropdb.sh t_v1d_schema
//! ```

use std::path::Path;

use plane_db::Query;

struct Catalog {
    domain: &'static str,
    source: &'static str,
    queries: &'static [&'static Query],
}

fn catalogs() -> Vec<Catalog> {
    vec![
        Catalog {
            domain: "issues",
            source: "issues_queries.rs",
            queries: plane_http_public::issues_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issue_subresources",
            source: "issue_subresources_queries.rs",
            queries: plane_http_public::issue_subresources_queries::ALL_QUERIES,
        },
    ]
}

fn collect_query_files(root: &Path, relative: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(root).expect("read_dir under src/");
    for entry in entries {
        let entry = entry.expect("directory entry");
        let name = entry.file_name();
        let path = entry.path();
        let relative = relative.join(&name);
        if path.is_dir() {
            collect_query_files(&path, &relative, out);
        } else if name.to_string_lossy().ends_with("queries.rs") {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn every_query_catalog_is_registered() {
    let mut discovered = Vec::new();
    collect_query_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        Path::new(""),
        &mut discovered,
    );
    discovered.sort();
    let mut listed: Vec<&str> = catalogs().iter().map(|catalog| catalog.source).collect();
    listed.sort_unstable();
    for source in &discovered {
        assert!(
            listed.contains(&source.as_str()),
            "src/{source} declares a catalog but tests/schema_check.rs does not list it"
        );
    }
    for source in &listed {
        assert!(
            discovered.iter().any(|discovered| discovered == source),
            "tests/schema_check.rs lists {source} but no such file exists under src/"
        );
    }
}

#[test]
fn query_names_are_unique_across_the_crate() {
    let mut seen: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for catalog in catalogs() {
        for query in catalog.queries {
            if let Some(first) = seen.insert(query.name, catalog.domain) {
                panic!(
                    "query name {:?} is declared in both {first} and {}",
                    query.name, catalog.domain
                );
            }
        }
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
        .flat_map(|catalog| catalog.queries.iter().copied())
        .collect();
    match plane_db::schema_check::verify_url(&url, &queries).await {
        Ok(report) => println!(
            "schema_check: {} statements verified against {}",
            report.checked, report.target
        ),
        Err(error) => panic!("{error}"),
    }
}
