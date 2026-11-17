// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Database tests for the two OAuth writes that are SQL, not Redis:
//! `create_update_account`'s `accounts` upsert (`adapter/oauth.py:101-132`)
//! and `validate_oauth_state`'s session read/delete
//! (`utils/oauth_state.py:9-19`).
//!
//! Both need the Django-migrated schema database:
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_ap0b_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_ap0b_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-auth --test oauth_db
//! ```
//!
//! Without `PLANE_RS_TEST_DATABASE_URL` the tests print a notice and pass, like
//! `schema_check`.

use std::str::FromStr;

use deadpool_postgres::Client;
use plane_auth::auth_flow::{TokenData, create_update_account, validate_oauth_state};
use plane_auth::json::Json;
use plane_auth::session::SessionStore;
use plane_core::Settings;
use plane_core::config::{EnvMap, Role};
use serde_json::{Value, json};
use uuid::Uuid;

async fn client() -> Option<Client> {
    let url = match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "oauth_db: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return None;
        }
        Err(message) => panic!("{message}"),
    };
    let config = tokio_postgres::Config::from_str(&url).expect("Postgres URL");
    let manager = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(2)
        .build()
        .expect("pool");
    Some(pool.get().await.expect("connection"))
}

/// A user row for `accounts.user_id`'s foreign key. The schema database is a
/// migration-only clone (it has no fixture rows), so the test creates its own
/// and deletes it again.
async fn create_user(client: &Client) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO users (id, password, username, email, first_name, last_name, \
             avatar, date_joined, created_at, updated_at, last_location, created_location, \
             is_superuser, is_managed, is_password_expired, is_active, is_staff, \
             is_email_verified, is_password_autoset, token, user_timezone, last_login_ip, \
             last_logout_ip, last_login_medium, last_login_uagent, is_bot, display_name, \
             is_email_valid, is_password_reset_required) \
             VALUES ($1, 'x', $2, $3, '', '', '', $4, $4, $4, '', '', FALSE, FALSE, FALSE, \
             TRUE, FALSE, FALSE, TRUE, $5, 'UTC', '', '', 'email', '', FALSE, '', FALSE, FALSE)",
            &[
                &id,
                &Uuid::new_v4().simple().to_string(),
                &format!("ap0b-{id}@example.com"),
                &now,
                &Uuid::new_v4().simple().to_string(),
            ],
        )
        .await
        .expect("insert a user");
    id
}

fn token_data(access_token: Value, refresh_token: Value) -> TokenData {
    TokenData::new(access_token, refresh_token, json!(""))
}

#[tokio::test]
async fn the_account_upsert_matches_django() {
    let Some(client) = client().await else {
        return;
    };
    let user_id = create_user(&client).await;
    let provider_id = Uuid::new_v4().simple().to_string();
    let user_data = json!({"email": "a@b.com", "user": {"provider_id": provider_id}});

    // ---- create ---------------------------------------------------------
    create_update_account(
        &client,
        "gitlab",
        &user_data,
        &token_data(json!("token-1"), Value::Null),
        user_id,
    )
    .await
    .expect("insert");

    let row = client
        .query_one(
            "SELECT id, user_id, provider_account_id, provider, access_token, \
             refresh_token, access_token_expired_at, id_token, metadata, created_at, updated_at \
             FROM accounts WHERE provider = 'gitlab' AND provider_account_id = $1",
            &[&provider_id],
        )
        .await
        .expect("the row exists");
    let id: Uuid = row.get(0);
    assert_eq!(row.get::<_, Uuid>(1), user_id);
    assert_eq!(row.get::<_, String>(2), provider_id);
    assert_eq!(row.get::<_, String>(3), "gitlab");
    assert_eq!(row.get::<_, String>(4), "token-1");
    assert_eq!(row.get::<_, Option<String>>(5), None);
    assert_eq!(row.get::<_, Option<chrono::DateTime<chrono::Utc>>>(6), None);
    // `token_response.get("id_token", "")`.
    assert_eq!(row.get::<_, String>(7), "");
    // `metadata`'s model default.
    assert_eq!(row.get::<_, serde_json::Value>(8), json!({}));
    let created_at: chrono::DateTime<chrono::Utc> = row.get(9);
    let updated_at: chrono::DateTime<chrono::Utc> = row.get(10);
    assert!(updated_at >= created_at);
    // The access token coming from a non-string JSON value is `str(value)`.
    assert!(row.get::<_, Option<String>>(5).is_none());

    // ---- update ---------------------------------------------------------
    create_update_account(
        &client,
        "gitlab",
        &user_data,
        &token_data(json!("token-2"), json!("refresh-2")),
        user_id,
    )
    .await
    .expect("update");
    let row = client
        .query_one(
            "SELECT id, access_token, refresh_token, created_at, updated_at \
             FROM accounts WHERE provider = 'gitlab' AND provider_account_id = $1",
            &[&provider_id],
        )
        .await
        .expect("row");
    assert_eq!(row.get::<_, Uuid>(0), id, "the update reused the row");
    assert_eq!(row.get::<_, String>(1), "token-2");
    assert_eq!(
        row.get::<_, Option<String>>(2).as_deref(),
        Some("refresh-2")
    );
    assert_eq!(
        row.get::<_, chrono::DateTime<chrono::Utc>>(3),
        created_at,
        "auto_now_add does not re-fire"
    );
    assert!(row.get::<_, chrono::DateTime<chrono::Utc>>(4) >= updated_at);

    // ---- a NOT NULL violation is swallowed ------------------------------
    // `access_token=None` is a `DatabaseError`, which `oauth.py:131-132`
    // logs and ignores; the row keeps its previous value.
    create_update_account(
        &client,
        "gitlab",
        &user_data,
        &token_data(Value::Null, Value::Null),
        user_id,
    )
    .await
    .expect("a failed account update is swallowed");
    let row = client
        .query_one(
            "SELECT access_token FROM accounts WHERE provider = 'gitlab' \
             AND provider_account_id = $1",
            &[&provider_id],
        )
        .await
        .expect("row");
    assert_eq!(row.get::<_, String>(0), "token-2");

    // A missing `provider_id` is also a NOT NULL violation (no row is
    // created).
    let before: i64 = client
        .query_one(
            "SELECT count(*) FROM accounts WHERE user_id = $1",
            &[&user_id],
        )
        .await
        .expect("count")
        .get(0);
    create_update_account(
        &client,
        "gitlab",
        &json!({"email": "a@b.com", "user": {}}),
        &token_data(json!("token-3"), Value::Null),
        user_id,
    )
    .await
    .expect("swallowed");
    let after: i64 = client
        .query_one(
            "SELECT count(*) FROM accounts WHERE user_id = $1",
            &[&user_id],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(before, after);

    // A missing `"user"` is the uncaught `AttributeError`, i.e. a 500.
    let error = create_update_account(
        &client,
        "gitlab",
        &json!({"email": "a@b.com"}),
        &token_data(json!("token-4"), Value::Null),
        user_id,
    )
    .await
    .expect_err("no user object");
    assert!(format!("{error:?}").contains("AttributeError"));

    client
        .execute("DELETE FROM accounts WHERE id = $1", &[&id])
        .await
        .expect("cleanup");
    client
        .execute("DELETE FROM users WHERE id = $1", &[&user_id])
        .await
        .expect("cleanup the user");
}

fn settings() -> Settings {
    let env = EnvMap::new()
        .set("REDIS_URL", "redis://127.0.0.1:6379/0")
        .set("WEB_URL", "http://localhost")
        .set("SESSION_COOKIE_AGE", "604800")
        .set("ADMIN_SESSION_COOKIE_AGE", "3600");
    Settings::from_source(&env, Role::Api).expect("settings")
}

#[tokio::test]
async fn the_state_check_deletes_only_on_success() {
    let Some(client) = client().await else {
        return;
    };
    let settings = settings();
    let secret_key = "ap0b-test-secret-key";
    let state = "0123456789abcdef0123456789abcdef";

    let mut store = SessionStore::new(None);
    store
        .set(&client, secret_key, "state", Json::Str(state.to_string()))
        .await
        .expect("store the state");
    store
        .save(&client, &settings, secret_key, true)
        .await
        .expect("create the session");
    let session_key = store.session_key().expect("a key").to_string();

    // A mismatch keeps the stored state, so the login can be retried. The
    // response phase saves the store (`SessionMiddleware`), which is what
    // persists the deletion below.
    assert!(
        !validate_oauth_state(&mut store, &client, secret_key, Some("wrong"))
            .await
            .expect("mismatch")
    );
    store
        .save(&client, &settings, secret_key, false)
        .await
        .expect("save after the mismatch");
    let mut reloaded = SessionStore::new(Some(session_key.clone()));
    assert_eq!(
        reloaded
            .get(&client, secret_key, "state")
            .await
            .expect("reload"),
        Some(Json::Str(state.to_string()))
    );

    // A missing state parameter is also a mismatch.
    assert!(
        !validate_oauth_state(&mut store, &client, secret_key, None)
            .await
            .expect("missing")
    );
    // An empty one too.
    assert!(
        !validate_oauth_state(&mut store, &client, secret_key, Some(""))
            .await
            .expect("empty")
    );

    // A match removes it, so a replay fails.
    assert!(
        validate_oauth_state(&mut store, &client, secret_key, Some(state))
            .await
            .expect("match")
    );
    store
        .save(&client, &settings, secret_key, false)
        .await
        .expect("save after the match");
    assert!(
        !validate_oauth_state(&mut store, &client, secret_key, Some(state))
            .await
            .expect("replay")
    );
    let mut reloaded = SessionStore::new(Some(session_key));
    assert_eq!(
        reloaded
            .get(&client, secret_key, "state")
            .await
            .expect("reload"),
        None
    );

    store.delete(&client).await.expect("delete the session row");
}
