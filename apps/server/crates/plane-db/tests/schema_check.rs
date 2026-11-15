// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check` for `plane-db`'s own catalog.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_skeleton_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_skeleton_schema \
//!   cargo test -p plane-db --test schema_check
//! apps/server/dev/dropdb.sh t_skeleton_schema
//! ```
//!
//! Without `PLANE_RS_TEST_DATABASE_URL` the test prints a notice and passes.
//! With `PLANE_RS_REQUIRE_DB_TESTS=1` it fails instead, so CI cannot skip the
//! check by accident. The rules live in `plane_db::schema_check`.

use plane_db::queries::ALL_QUERIES;

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

    // The hand-written statements plus the generated `SELECT … LIMIT 0` for
    // every model row type.
    let mut queries: Vec<&plane_db::Query> = ALL_QUERIES.to_vec();
    queries.extend_from_slice(plane_db::models::catalog::ALL_MODEL_QUERIES);

    match plane_db::schema_check::verify_url(&url, &queries).await {
        Ok(report) => println!(
            "schema_check: {} statements verified against {}",
            report.checked, report.target
        ),
        Err(error) => panic!("{error}"),
    }
}
