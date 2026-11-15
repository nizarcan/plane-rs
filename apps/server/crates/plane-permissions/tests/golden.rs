// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Differential test: replay `tests/golden/matrix.json` produced by
//! `apps/server/tools/permissions/oracle.py` against the fixture database.
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_perms_fixture --replace
//! PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \
//!   apps/api/.venv/bin/python apps/server/tools/permissions/fixture.py
//! PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \
//!   apps/api/.venv/bin/python apps/server/tools/permissions/oracle.py
//!
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_perms_fixture \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-permissions --test golden
//! ```
//!
//! The golden file is the specification: when a case disagrees, the Rust code
//! is wrong (or the oracle's regenerated fixture changed, in which case the
//! whole matrix is regenerated and reviewed).

use std::str::FromStr;

use http::Method;
use plane_auth::{AuthUser, RequestUser};
use plane_permissions::{
    AllowPermission, CheckFailure, CreatorModel, Decision, Denial, Error, Level, PermCtx,
    PermissionClass, Role, UrlKwargs, allow_permission, check_permissions,
    dynamic_permission_classes, has_permission, issue_hidden_from_guest,
};
use serde_json::{Value, json};
use uuid::Uuid;

const ROLES_NONE: &[Role] = &[];
const ROLES_ADMIN: &[Role] = &[Role::Admin];
const ROLES_ADMIN_MEMBER: &[Role] = &[Role::Admin, Role::Member];
const ROLES_ADMIN_MEMBER_GUEST: &[Role] = &[Role::Admin, Role::Member, Role::Guest];

fn roles_for(values: &Value) -> &'static [Role] {
    let mut roles: Vec<i16> = values
        .as_array()
        .expect("roles array")
        .iter()
        .map(|value| value.as_i64().expect("role number") as i16)
        .collect();
    roles.sort_unstable();
    match roles.as_slice() {
        [] => ROLES_NONE,
        [20] => ROLES_ADMIN,
        [15, 20] => ROLES_ADMIN_MEMBER,
        [5, 15, 20] => ROLES_ADMIN_MEMBER_GUEST,
        other => panic!("unexpected role set {other:?} in the golden specs"),
    }
}

fn request_user(entry: &Value) -> RequestUser {
    if entry["anonymous"].as_bool().expect("anonymous flag") {
        return RequestUser::Anonymous;
    }
    let id = entry["id"].as_str().expect("user id");
    RequestUser::from_user(AuthUser {
        id: Uuid::parse_str(id).expect("user uuid"),
        email: String::new(),
        username: String::new(),
        display_name: String::new(),
        first_name: String::new(),
        last_name: String::new(),
        user_timezone: "UTC".to_string(),
        is_active: true,
        is_superuser: false,
        is_staff: false,
        is_bot: false,
        is_email_verified: true,
        password: "!".to_string(),
    })
}

/// The tag the matrix records for an unexpected exception from a check.
fn error_tag(error: &Error) -> String {
    match error {
        Error::MissingKeyword(_) => "exception:KeyError".to_string(),
        Error::UnpreparedUser | Error::InvalidUuid { .. } => {
            "exception:ValidationError".to_string()
        }
        Error::Db(_) => "exception:DatabaseError".to_string(),
    }
}

fn method(value: &str) -> Method {
    Method::from_bytes(value.as_bytes()).expect("HTTP method")
}

fn outcome_bool(result: Result<bool, Error>) -> Value {
    match result {
        Ok(value) => json!(value),
        Err(error) => json!(error_tag(&error)),
    }
}

fn outcome_spec(result: Result<Decision, Error>) -> Value {
    match result {
        Ok(Decision::Allow) => json!(true),
        Ok(Decision::Deny(_)) => json!("deny"),
        Err(error) => json!(error_tag(&error)),
    }
}

fn outcome_check(result: Result<(), CheckFailure>) -> Value {
    match result {
        Ok(()) => json!(true),
        Err(CheckFailure::Denied(Denial::NotAuthenticated)) => json!("not_authenticated"),
        Err(CheckFailure::Denied(Denial::PermissionDenied { .. })) => json!("permission_denied"),
        Err(CheckFailure::Query(error)) => json!(error_tag(&error)),
    }
}

#[tokio::test]
async fn golden_matrix() {
    let url = match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "golden: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return;
        }
        Err(message) => panic!("{message}"),
    };

    let golden: Value = serde_json::from_str(include_str!("golden/matrix.json"))
        .expect("tests/golden/matrix.json is valid JSON");
    let results = golden["results"].as_object().expect("results object");
    let contexts = golden["contexts"].as_object().expect("contexts object");
    let users = golden["users"].as_object().expect("users object");
    let specs = golden["specs"].as_array().expect("specs array");
    let class_lists = golden["class_lists"].as_array().expect("class_lists array");
    let details = golden["details"].as_object().expect("details object");

    // The deny body is part of the contract; assert the interned value once.
    let deny = &details["deny"].as_array().expect("deny details")[0];
    assert_eq!(deny["status"].as_u64(), Some(403), "deny status");
    assert_eq!(
        deny["body"],
        json!({"error": "You don't have the required permissions."}),
        "deny body"
    );

    // A pool rather than a single connection: `PermCtx` takes a
    // `deadpool_postgres::Client`, the same type the API hands to a handler.
    let config = tokio_postgres::Config::from_str(&url)
        .expect("PLANE_RS_TEST_DATABASE_URL is a Postgres URL");
    let manager = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(2)
        .build()
        .expect("pool");
    let client = pool
        .get()
        .await
        .expect("connection to the fixture database");

    let mut checked = 0_usize;
    let mut mismatches: Vec<String> = Vec::new();

    for (key, expected) in results {
        let parts: Vec<&str> = key.split('|').collect();
        assert_eq!(parts.len(), 6, "case key {key:?} has six fields");
        let (kind, subject, context_name, user_name, method_name, action) =
            (parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]);

        let context = contexts
            .get(context_name)
            .unwrap_or_else(|| panic!("unknown context {context_name:?} in {key:?}"));
        let kwargs_map = context["kwargs"].as_object().expect("context kwargs");
        let kwargs: UrlKwargs<'_> = kwargs_map
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str().expect("kwarg string")))
            .collect();
        let url_name = context["url_name"].as_str();
        let project_identifier = context["view_project_identifier"].as_str();
        let user = request_user(
            users
                .get(user_name)
                .unwrap_or_else(|| panic!("unknown user {user_name:?} in {key:?}")),
        );
        let method_value = method(method_name);
        let action = if action == "-" { None } else { Some(action) };
        let ctx = PermCtx::new(&client, &user, &method_value, &kwargs)
            .with_action(action)
            .with_url_name(url_name)
            .with_project_identifier(project_identifier);

        let actual = match kind {
            "class" => {
                let class = PermissionClass::from_dotted(subject)
                    .unwrap_or_else(|| panic!("unknown permission class {subject:?}"));
                outcome_bool(has_permission(class, &ctx).await)
            }
            "spec" => {
                let index: usize = subject.parse().expect("spec index");
                let spec = &specs[index];
                let model = spec["model"].as_str().map(|dotted| {
                    CreatorModel::from_dotted(dotted)
                        .unwrap_or_else(|| panic!("unknown creator model {dotted:?}"))
                });
                let allow = AllowPermission::new(
                    roles_for(&spec["roles"]),
                    Level::parse(spec["level"].as_str().expect("spec level")),
                    spec["creator"].as_bool().expect("spec creator"),
                    model,
                );
                let outcome = allow_permission(&allow, &ctx).await;
                if let Ok(Decision::Deny(response)) = outcome {
                    assert_eq!(response.status, 403, "deny status for {key:?}");
                    assert_eq!(
                        response.error, "You don't have the required permissions.",
                        "deny body for {key:?}"
                    );
                }
                outcome_spec(outcome)
            }
            "helper" => {
                let slug = kwargs.get("slug").expect("helper slug");
                let project_id = kwargs.get("project_id").expect("helper project_id");
                let issue_id = kwargs.get("pk").expect("helper pk");
                outcome_bool(issue_hidden_from_guest(&ctx, slug, project_id, issue_id).await)
            }
            "check" => {
                let index: usize = subject.parse().expect("class list index");
                let classes: Vec<PermissionClass> = class_lists[index]
                    .as_array()
                    .expect("class list")
                    .iter()
                    .map(|path| {
                        let path = path.as_str().expect("class path");
                        PermissionClass::from_dotted(path)
                            .unwrap_or_else(|| panic!("unknown permission class {path:?}"))
                    })
                    .collect();
                outcome_check(check_permissions(&classes, &ctx).await)
            }
            other => panic!("unknown case kind {other:?} in {key:?}"),
        };

        checked += 1;
        if &actual != expected {
            mismatches.push(format!("{key}: expected {expected}, got {actual}"));
        }
    }

    // `get_permissions` overrides: the resolved class list per (view, method,
    // action), independent of the user.
    let mut dynamic_checked = 0_usize;
    for entry in golden["dynamic"].as_array().expect("dynamic array") {
        let view = entry["view"].as_str().expect("dynamic view");
        let method_value = method(entry["method"].as_str().expect("dynamic method"));
        let action = entry["action"].as_str();
        let actual: Vec<&str> = dynamic_permission_classes(view, &method_value, action)
            .unwrap_or_else(|| panic!("no dynamic permission rule for {view}"))
            .iter()
            .map(|class| class.dotted())
            .collect();
        let expected: Vec<&str> = entry["classes"]
            .as_array()
            .expect("dynamic classes")
            .iter()
            .map(|class| class.as_str().expect("class path"))
            .collect();
        dynamic_checked += 1;
        if actual != expected {
            mismatches.push(format!(
                "dynamic {view} {} {:?}: expected {expected:?}, got {actual:?}",
                method_value, action
            ));
        }
    }

    // Every class named in `classes` must resolve structurally, even if a
    // context list were missing.
    for path in golden["classes"].as_array().expect("classes array") {
        let path = path.as_str().expect("class path");
        assert!(
            PermissionClass::from_dotted(path).is_some(),
            "routes.json names a class this crate cannot resolve: {path}"
        );
    }

    assert!(
        mismatches.is_empty(),
        "{} of {} golden cases disagree (first 20):\n{}",
        mismatches.len(),
        checked + dynamic_checked,
        mismatches
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );

    println!(
        "golden: {checked} permission cases and {dynamic_checked} dynamic resolutions match; \
         database {}",
        golden["database"].as_str().unwrap_or("?")
    );
}
