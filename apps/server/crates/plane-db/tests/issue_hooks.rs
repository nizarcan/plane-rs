// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DB-backed tests for H10/H11 (`hooks::issue`) and the m2m writers
//! (`hooks::issue_m2m`).
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_rsp5w_test --template plane_fix_tpl
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_rsp5w_test \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-db --test issue_hooks
//! ```
//!
//! The clone must be the fixture database (`plane_fix_tpl`): the tests use its
//! `ALP` project (`b0063033…`), workspace, states and users. Rows they create
//! stay in the clone; assertions are relative, so a reuse is harmless.

use std::sync::OnceLock;
use std::time::Duration;

use chrono::{DateTime, Utc};
use plane_db::JsonText;
use plane_db::hooks::{issue as issue_hooks, issue_m2m};
use plane_db::models::issue::Issue;
use plane_db::schema_check::test_database_url;
use std::str::FromStr;

use deadpool_postgres::Client;
use tokio::sync::Mutex;
use uuid::Uuid;

/// The tests share one fixture project (and therefore one advisory lock and
/// one sequence counter), so they run one at a time. The concurrency test's
/// two creates still race each other; only the test *functions* are serialised.
fn db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct TestDatabase {
    pool: deadpool_postgres::Pool,
}

impl TestDatabase {
    async fn client(&self) -> Client {
        self.pool
            .get()
            .await
            .expect("connection to the fixture database")
    }
}

async fn database() -> Option<TestDatabase> {
    let url = match test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "issue_hooks: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
                 set PLANE_RS_REQUIRE_DB_TESTS=1 to make this a failure)"
            );
            return None;
        }
        Err(message) => panic!("{message}"),
    };
    let config = tokio_postgres::Config::from_str(&url)
        .expect("PLANE_RS_TEST_DATABASE_URL is a Postgres URL");
    let manager = deadpool_postgres::Manager::new(config, tokio_postgres::NoTls);
    let pool = deadpool_postgres::Pool::builder(manager)
        .max_size(4)
        .build()
        .expect("pool");
    Some(TestDatabase { pool })
}

/// The fixture project, workspace and an existing user.
async fn fixture(client: &Client) -> (Uuid, Uuid, Option<Uuid>) {
    let row = client
        .query_one(
            "SELECT id, workspace_id, created_by_id FROM projects \
             WHERE identifier = 'ALP' AND deleted_at IS NULL LIMIT 1",
            &[],
        )
        .await
        .expect("fixture project ALP");
    (row.get(0), row.get(1), row.get(2))
}

async fn users(client: &Client, count: i64) -> Vec<Uuid> {
    let rows = client
        .query("SELECT id FROM users ORDER BY id LIMIT $1", &[&count])
        .await
        .expect("fixture users");
    rows.iter().map(|row| row.get(0)).collect()
}

fn now() -> DateTime<Utc> {
    Utc::now()
}

fn new_issue(project_id: Uuid, workspace_id: Uuid, name: &str) -> Issue {
    let now = now();
    Issue {
        created_at: now,
        updated_at: now,
        created_by_id: None,
        updated_by_id: None,
        deleted_at: None,
        id: Uuid::new_v4(),
        project_id,
        workspace_id,
        parent_id: None,
        state_id: None,
        point: None,
        estimate_point_id: None,
        name: name.to_owned(),
        description_json: JsonText::new("{}"),
        description_html: "<p></p>".to_owned(),
        description_stripped: None,
        description_binary: None,
        priority: "none".to_owned(),
        start_date: None,
        target_date: None,
        sequence_id: 1,
        sort_order: 65535.0,
        completed_at: None,
        archived_at: None,
        is_draft: false,
        external_source: None,
        external_id: None,
        type_id: None,
    }
}

async fn create(
    client: &Client,
    project_id: Uuid,
    workspace_id: Uuid,
    user: Option<Uuid>,
) -> Issue {
    let mut issue = new_issue(
        project_id,
        workspace_id,
        &format!("hook test {}", Uuid::new_v4()),
    );
    issue_hooks::create_issue_row(client, &mut issue, user, now())
        .await
        .expect("create_issue_row");
    issue
}

#[tokio::test]
async fn create_sets_sequence_sort_order_and_the_issue_sequence_row() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;

    let first = create(&client, project_id, workspace_id, user).await;
    let second = create(&client, project_id, workspace_id, user).await;

    assert_eq!(second.sequence_id, first.sequence_id + 1);
    assert_eq!(second.sort_order, first.sort_order + 10_000.0);
    assert!(first.state_id.is_some(), "H10a resolved the default state");

    let row = client
        .query_one(
            "SELECT sequence FROM issue_sequences WHERE issue_id = $1 AND deleted_at IS NULL",
            &[&second.id],
        )
        .await
        .expect("issue_sequences row");
    assert_eq!(row.get::<_, i64>(0), i64::from(second.sequence_id));
}

#[tokio::test]
async fn concurrent_creates_get_distinct_consecutive_sequences() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let first_client = db.client().await;
    let second_client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&first_client).await;

    let mut first_issue = new_issue(
        project_id,
        workspace_id,
        &format!("concurrent {}", Uuid::new_v4()),
    );
    let mut second_issue = new_issue(
        project_id,
        workspace_id,
        &format!("concurrent {}", Uuid::new_v4()),
    );
    let first = issue_hooks::create_issue_row(&first_client, &mut first_issue, user, now());
    let second = issue_hooks::create_issue_row(&second_client, &mut second_issue, user, now());
    let (first, second) = tokio::join!(first, second);
    first.expect("first create");
    second.expect("second create");

    let mut sequences = [first_issue.sequence_id, second_issue.sequence_id];
    sequences.sort_unstable();
    assert_ne!(sequences[0], sequences[1]);
    assert_eq!(sequences[1], sequences[0] + 1);
}

#[tokio::test]
async fn a_django_style_lock_holder_blocks_the_create() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let lock_client = db.client().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&lock_client).await;

    lock_client.batch_execute("BEGIN").await.expect("begin");
    lock_client
        .execute(
            "SELECT pg_advisory_xact_lock($1)",
            &[&issue_hooks::advisory_lock_key(project_id)],
        )
        .await
        .expect("lock");

    let mut issue = new_issue(
        project_id,
        workspace_id,
        &format!("locked {}", Uuid::new_v4()),
    );
    let create = issue_hooks::create_issue_row(&client, &mut issue, user, now());
    tokio::pin!(create);
    tokio::select! {
        result = &mut create => panic!("create completed while the advisory lock was held: {result:?}"),
        () = tokio::time::sleep(Duration::from_millis(300)) => {}
    }

    lock_client.batch_execute("COMMIT").await.expect("release");
    create.await.expect("create after release");
}

async fn live_assignees(client: &Client, issue_id: Uuid) -> i64 {
    let row = client
        .query_one(
            "SELECT COUNT(*) FROM issue_assignees \
             WHERE issue_id = $1 AND deleted_at IS NULL",
            &[&issue_id],
        )
        .await
        .expect("count");
    row.get(0)
}

#[tokio::test]
async fn re_adding_a_soft_deleted_assignee_keeps_one_live_row() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let assignee = users(&client, 1).await[0];

    issue_m2m::replace_assignees(&client, &issue, &[assignee], now())
        .await
        .expect("first set");
    issue_m2m::replace_assignees(&client, &issue, &[], now())
        .await
        .expect("clear");
    issue_m2m::replace_assignees(&client, &issue, &[assignee], now())
        .await
        .expect("re-add");

    assert_eq!(live_assignees(&client, issue.id).await, 1);
    let total: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1",
            &[&issue.id],
        )
        .await
        .expect("total")
        .get(0);
    assert_eq!(total, 2, "one soft-deleted row and one live row");
}

#[tokio::test]
async fn a_duplicate_in_the_input_writes_one_live_row() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let assignee = users(&client, 1).await[0];

    issue_m2m::replace_assignees(&client, &issue, &[assignee, assignee], now())
        .await
        .expect("replace");

    assert_eq!(live_assignees(&client, issue.id).await, 1);
}

#[tokio::test]
async fn eleven_assignees_cross_the_ten_row_batch_boundary() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let assignees = users(&client, 11).await;
    assert_eq!(assignees.len(), 11, "fixture has at least 11 users");

    issue_m2m::insert_assignees(&client, &issue, &assignees, now())
        .await
        .expect("insert 11");

    assert_eq!(live_assignees(&client, issue.id).await, 11);
}

#[tokio::test]
async fn a_duplicate_in_the_second_assignee_batch_rolls_back_every_batch() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let assignees = users(&client, 11).await;
    // The first batch of ten is unique; the second batch repeats one of them,
    // so its partial unique index raises 23505. `bulk_create` is one atomic, so
    // the serializer's `except IntegrityError: pass` leaves no row at all.
    let mut ids = assignees[..10].to_vec();
    ids.push(assignees[0]);

    issue_m2m::insert_assignees(&client, &issue, &ids, now())
        .await
        .expect("the serializer swallows the IntegrityError");

    assert_eq!(live_assignees(&client, issue.id).await, 0);
    let total: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM issue_assignees WHERE issue_id = $1",
            &[&issue.id],
        )
        .await
        .expect("total")
        .get(0);
    assert_eq!(
        total, 0,
        "the whole call rolled back, not just the second batch"
    );
}

async fn labels(client: &Client, count: i64) -> Vec<Uuid> {
    let rows = client
        .query("SELECT id FROM labels ORDER BY id LIMIT $1", &[&count])
        .await
        .expect("fixture labels");
    rows.iter().map(|row| row.get(0)).collect()
}

async fn live_labels(client: &Client, issue_id: Uuid) -> i64 {
    let row = client
        .query_one(
            "SELECT COUNT(*) FROM issue_labels \
             WHERE issue_id = $1 AND deleted_at IS NULL",
            &[&issue_id],
        )
        .await
        .expect("count");
    row.get(0)
}

#[tokio::test]
async fn eleven_labels_cross_the_ten_row_batch_boundary() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let label_ids = labels(&client, 11).await;
    assert_eq!(label_ids.len(), 11, "fixture has at least 11 labels");

    issue_m2m::insert_labels(&client, &issue, &label_ids, now())
        .await
        .expect("insert 11");

    assert_eq!(live_labels(&client, issue.id).await, 11);
}

#[tokio::test]
async fn issue_labels_have_no_unique_constraint_so_a_duplicate_is_inserted() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let label_ids = labels(&client, 11).await;
    // `IssueLabel` has no unique index (`MODEL_HOOKS.md` §2.9), unlike
    // `IssueAssignee`; the same duplicate in the second batch is accepted.
    let mut ids = label_ids[..10].to_vec();
    ids.push(label_ids[0]);

    issue_m2m::insert_labels(&client, &issue, &ids, now())
        .await
        .expect("no constraint to violate");

    assert_eq!(live_labels(&client, issue.id).await, 11);
}

#[tokio::test]
async fn a_failing_label_batch_rolls_back_every_batch_and_propagates() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, workspace_id, user) = fixture(&client).await;
    let issue = create(&client, project_id, workspace_id, user).await;
    let mut ids = labels(&client, 10).await;
    // A missing label in the second batch is a foreign-key violation (23503),
    // not the swallowed 23505: the call rolls back and returns the error.
    ids.push(Uuid::new_v4());

    let result = issue_m2m::insert_labels(&client, &issue, &ids, now()).await;

    assert!(result.is_err(), "a non-unique error propagates");
    assert_eq!(live_labels(&client, issue.id).await, 0);
}

#[tokio::test]
async fn default_assignee_requires_an_active_project_member_with_role_15() {
    let Some(db) = database().await else { return };
    let _guard = db_lock().lock().await;
    let client = db.client().await;
    let (project_id, _, _) = fixture(&client).await;

    // `fix.member@example.com` is an active project member with role 15;
    // `fix.guest@example.com` is role 5.
    let member: Uuid = client
        .query_one(
            "SELECT member_id FROM project_members \
             WHERE project_id = $1 AND role >= 15 AND is_active AND deleted_at IS NULL \
             LIMIT 1",
            &[&project_id],
        )
        .await
        .expect("project member")
        .get(0);
    let guest: Uuid = client
        .query_one(
            "SELECT member_id FROM project_members \
             WHERE project_id = $1 AND role < 15 AND is_active AND deleted_at IS NULL \
             LIMIT 1",
            &[&project_id],
        )
        .await
        .expect("project guest")
        .get(0);

    assert_eq!(
        issue_m2m::default_assignee(&client, project_id, Some(member))
            .await
            .expect("member"),
        Some(member)
    );
    assert_eq!(
        issue_m2m::default_assignee(&client, project_id, Some(guest))
            .await
            .expect("guest"),
        None
    );
    assert_eq!(
        issue_m2m::default_assignee(&client, project_id, None)
            .await
            .expect("none"),
        None
    );
}
