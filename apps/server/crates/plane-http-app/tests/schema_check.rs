// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check` for every query catalog in this crate.
//!
//! Each domain owns `src/<domain>/queries.rs` with its own `ALL_QUERIES`; this
//! test is the one place that prepares them all, so a domain agent adds
//! statements without touching a shared file. `every_query_catalog_is_registered`
//! walks `src/` and fails when a `*queries.rs` file is missing from [`catalogs`],
//! so a new domain cannot forget to register its catalog (and a renamed file
//! cannot leave a stale entry).
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

use std::path::Path;

use plane_db::Query;
use plane_http_app::common::queries as common;

// `issues::sub_queries` lives under the crate-private `issues::sub_resources`
// module, so an integration test cannot name it. Compiling the file here gives
// the test the same catalog values the handlers use; the module path is
// registered through `source` below like every other catalog.
#[path = "../src/issues/sub_queries.rs"]
mod issues_sub_queries;

/// One catalog and the file that declares it.
struct Catalog {
    /// Name used in the duplicate check and in a failure message.
    domain: &'static str,
    /// Path relative to `src/`, as [`every_query_catalog_is_registered`] sees it.
    source: &'static str,
    queries: &'static [&'static Query],
}

/// Every catalog this crate owns, in `src/lib.rs`'s registration order.
fn catalogs() -> Vec<Catalog> {
    vec![
        Catalog {
            domain: "common",
            source: "common/queries.rs",
            queries: common::ALL_QUERIES,
        },
        Catalog {
            domain: "meta",
            source: "meta/queries.rs",
            queries: plane_http_app::meta::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "users_me",
            source: "users_me/queries.rs",
            queries: plane_http_app::users_me::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "assets",
            source: "assets/queries.rs",
            queries: plane_http_app::assets::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "workspaces",
            source: "workspaces/queries.rs",
            queries: plane_http_app::workspaces::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "analytics",
            source: "analytics/queries.rs",
            queries: plane_http_app::analytics::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "projects",
            source: "projects/queries.rs",
            queries: plane_http_app::projects::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "projects_write",
            source: "projects/write_queries.rs",
            queries: plane_http_app::projects::write_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "projects_member",
            source: "projects/member_queries.rs",
            queries: plane_http_app::projects::member_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "states",
            source: "states/queries.rs",
            queries: plane_http_app::states::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "labels",
            source: "labels/queries.rs",
            queries: plane_http_app::labels::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "estimates",
            source: "estimates/queries.rs",
            queries: plane_http_app::estimates::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues",
            source: "issues/queries.rs",
            queries: plane_http_app::issues::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues_write_comment",
            source: "issues/write_comment_queries.rs",
            queries: plane_http_app::issues::write_comment_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues_draft",
            source: "issues/write_draft_queries.rs",
            queries: plane_http_app::issues::write_draft_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues_sub",
            source: "issues/sub_queries.rs",
            queries: issues_sub_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues_write_sub",
            source: "issues/write_sub_queries.rs",
            queries: plane_http_app::issues::write_sub_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "issues_write",
            source: "issues/write_issue_queries.rs",
            queries: plane_http_app::issues::write_issue_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "cycles_write_issue",
            source: "cycles/write_issue_queries.rs",
            queries: plane_http_app::cycles::write_issue_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "cycles",
            source: "cycles/queries.rs",
            queries: plane_http_app::cycles::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "cycles_write",
            source: "cycles/write_queries.rs",
            queries: plane_http_app::cycles::write_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "modules",
            source: "modules/queries.rs",
            queries: plane_http_app::modules::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "modules_write",
            source: "modules/write_queries.rs",
            queries: plane_http_app::modules::write_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "modules_write_issue",
            source: "modules/write_issue_queries.rs",
            queries: plane_http_app::modules::write_issue_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "views",
            source: "views/queries.rs",
            queries: plane_http_app::views::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "views_write",
            source: "views/write_view_queries.rs",
            queries: plane_http_app::views::write_view_queries::ALL_QUERIES,
        },
        Catalog {
            domain: "pages",
            source: "pages/queries.rs",
            queries: plane_http_app::pages::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "intake",
            source: "intake/queries.rs",
            queries: plane_http_app::intake::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "notifications",
            source: "notifications/queries.rs",
            queries: plane_http_app::notifications::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "search",
            source: "search/queries.rs",
            queries: plane_http_app::search::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "stickies",
            source: "stickies/queries.rs",
            queries: plane_http_app::stickies::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "webhooks",
            source: "webhooks/queries.rs",
            queries: plane_http_app::webhooks::queries::ALL_QUERIES,
        },
        Catalog {
            domain: "ai",
            source: "ai/queries.rs",
            queries: plane_http_app::ai::queries::ALL_QUERIES,
        },
    ]
}

/// Append every `*queries.rs` file under `root`, as a `/`-separated path
/// relative to `src/`.
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
fn query_names_are_unique_across_the_crate() {
    // The catalog name is what `schema_check` prints and what a handler reads in
    // a stack trace, so two domains must not guess the same one.
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

#[test]
fn every_query_catalog_is_registered() {
    // A domain's catalog is a file, not a line in this list, so discovery has
    // to be by file: a new `src/<domain>/queries.rs` (or a second catalog such
    // as `src/issues/sub_queries.rs`) that is not registered here fails the
    // test instead of silently going unchecked.
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
