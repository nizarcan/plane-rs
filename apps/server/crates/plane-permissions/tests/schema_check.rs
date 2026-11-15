// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `schema_check` for this crate's catalog.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_perms_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_perms_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-permissions --test schema_check
//! apps/server/dev/dropdb.sh t_perms_schema
//! ```
//!
//! The skip/require rules live in `plane_db::schema_check`, so every crate
//! behaves the same.

use plane_permissions::queries::ALL_QUERIES;

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

    match plane_db::schema_check::verify_url(&url, ALL_QUERIES).await {
        Ok(report) => println!(
            "schema_check: {} statements verified against {}",
            report.checked, report.target
        ),
        Err(error) => panic!("{error}"),
    }
}
