// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Database tests for the public board resolution (`PUBLIC_INSTANCES.md` §2a):
//! the three lookups, their 404s and the three write-scope predicates.
//!
//! `public/board.rs` has no route yet (sets A/B register the handlers), so the
//! differential scenarios cannot reach it; this test executes each statement
//! against the Django-migrated schema and checks the answer the ORM would give.
//! It builds its own rows, so it runs on the migration-only template:
//!
//! ```sh
//! apps/server/dev/mkdb.sh t_pubp0_schema
//! cd apps/server
//! PLANE_RS_TEST_DATABASE_URL=postgresql://plane:plane@127.0.0.1:55432/t_pubp0_schema \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 cargo test -p plane-http-app --test public_board_db
//! ```
//!
//! Without `PLANE_RS_TEST_DATABASE_URL` the test prints a notice and passes,
//! like `schema_check`.

use std::str::FromStr;

use deadpool_postgres::Client;
use plane_core::error::ApiError;
use plane_drf::error::ViewError;
use plane_http_app::public::board;
use uuid::Uuid;

async fn client() -> Option<Client> {
    let url = match plane_db::schema_check::test_database_url() {
        Ok(Some(url)) => url,
        Ok(None) => {
            println!(
                "public_board_db: skipped (PLANE_RS_TEST_DATABASE_URL is unset; \
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

/// Every row this test owns, so teardown deletes exactly those.
#[derive(Debug, Default)]
struct Fixture {
    users: Vec<Uuid>,
    workspaces: Vec<Uuid>,
    projects: Vec<Uuid>,
    states: Vec<Uuid>,
    issues: Vec<Uuid>,
    comments: Vec<Uuid>,
    boards: Vec<Uuid>,
}

impl Fixture {
    async fn teardown(&self, client: &Client) {
        for id in &self.boards {
            client
                .execute("DELETE FROM deploy_boards WHERE id = $1", &[id])
                .await
                .expect("delete a board");
        }
        for id in &self.comments {
            client
                .execute("DELETE FROM issue_comments WHERE id = $1", &[id])
                .await
                .expect("delete a comment");
        }
        for id in &self.issues {
            client
                .execute("DELETE FROM issues WHERE id = $1", &[id])
                .await
                .expect("delete an issue");
        }
        for id in &self.states {
            client
                .execute("DELETE FROM states WHERE id = $1", &[id])
                .await
                .expect("delete a state");
        }
        for id in &self.projects {
            client
                .execute("DELETE FROM projects WHERE id = $1", &[id])
                .await
                .expect("delete a project");
        }
        for id in &self.workspaces {
            client
                .execute("DELETE FROM workspaces WHERE id = $1", &[id])
                .await
                .expect("delete a workspace");
        }
        for id in &self.users {
            client
                .execute("DELETE FROM users WHERE id = $1", &[id])
                .await
                .expect("delete a user");
        }
    }
}

async fn insert_user(client: &Client, fixture: &mut Fixture) -> Uuid {
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
                &format!("pubp0-{id}@example.com"),
                &now,
                &Uuid::new_v4().simple().to_string(),
            ],
        )
        .await
        .expect("insert a user");
    fixture.users.push(id);
    id
}

async fn insert_workspace(client: &Client, fixture: &mut Fixture, owner: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO workspaces (id, created_at, updated_at, name, slug, owner_id, \
             timezone, background_color) \
             VALUES ($1, $2, $2, 'pubp0', $3, $4, 'UTC', '#ffffff')",
            &[&id, &now, &format!("pubp0-{id}"), &owner],
        )
        .await
        .expect("insert a workspace");
    fixture.workspaces.push(id);
    id
}

async fn insert_project(
    client: &Client,
    fixture: &mut Fixture,
    workspace: Uuid,
    archived: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO projects (id, created_at, updated_at, name, description, network, \
             identifier, workspace_id, cycle_view, module_view, issue_views_view, page_view, \
             intake_view, archive_in, close_in, logo_props, is_time_tracking_enabled, \
             is_issue_type_enabled, guest_view_all_features, timezone, archived_at) \
             VALUES ($1, $2, $2, $6, '', 2, $3, $4, TRUE, TRUE, TRUE, TRUE, TRUE, 30, 30, \
             '{}', FALSE, FALSE, FALSE, 'UTC', $5)",
            &[
                &id,
                &now,
                &format!("PUBP0{}", &id.simple().to_string()[..6]),
                &workspace,
                &archived.then_some(now),
                &format!("pubp0-{id}"),
            ],
        )
        .await
        .expect("insert a project");
    fixture.projects.push(id);
    id
}

async fn insert_state(
    client: &Client,
    fixture: &mut Fixture,
    workspace: Uuid,
    project: Uuid,
    group: &str,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO states (id, created_at, updated_at, name, description, color, slug, \
             project_id, workspace_id, sequence, \"group\", \"default\", is_triage) \
             VALUES ($1, $2, $2, $3, '', '#000000', $3, $4, $5, 1000.0, $3, FALSE, $6)",
            &[
                &id,
                &now,
                &group,
                &project,
                &workspace,
                &(group == "triage"),
            ],
        )
        .await
        .expect("insert a state");
    fixture.states.push(id);
    id
}

#[allow(clippy::too_many_arguments)]
async fn insert_issue(
    client: &Client,
    fixture: &mut Fixture,
    workspace: Uuid,
    project: Uuid,
    state: Option<Uuid>,
    draft: bool,
    archived: bool,
    deleted: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, project_id, workspace_id, description_html, sort_order, is_draft, \
             state_id, archived_at, deleted_at) \
             VALUES ($1, $2, $2, 'pubp0', '{}', 'none', 1, $3, $4, '<p></p>', 1000.0, $5, $6, \
             $7, $8)",
            &[
                &id,
                &now,
                &project,
                &workspace,
                &draft,
                &state,
                // `issues.archived_at` is a `date`, not a timestamp.
                &archived.then_some(now.date_naive()),
                &deleted.then_some(now),
            ],
        )
        .await
        .expect("insert an issue");
    fixture.issues.push(id);
    id
}

#[allow(clippy::too_many_arguments)]
async fn insert_comment(
    client: &Client,
    fixture: &mut Fixture,
    workspace: Uuid,
    project: Uuid,
    issue: Uuid,
    actor: Uuid,
    access: &str,
    deleted: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO issue_comments (id, created_at, updated_at, comment_stripped, \
             attachments, issue_id, project_id, workspace_id, comment_html, comment_json, \
             access, actor_id, deleted_at) \
             VALUES ($1, $2, $2, '', '{}', $3, $4, $5, '<p></p>', '{}', $6, $7, $8)",
            &[
                &id,
                &now,
                &issue,
                &project,
                &workspace,
                &access,
                &actor,
                &deleted.then_some(now),
            ],
        )
        .await
        .expect("insert a comment");
    fixture.comments.push(id);
    id
}

#[allow(clippy::too_many_arguments)]
async fn insert_board(
    client: &Client,
    fixture: &mut Fixture,
    workspace: Uuid,
    project: Option<Uuid>,
    entity_identifier: Option<Uuid>,
    entity_name: &str,
    anchor: &str,
    deleted: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    client
        .execute(
            "INSERT INTO deploy_boards (id, created_at, updated_at, workspace_id, project_id, \
             entity_identifier, entity_name, anchor, is_comments_enabled, is_reactions_enabled, \
             is_votes_enabled, view_props, is_activity_enabled, is_disabled, deleted_at) \
             VALUES ($1, $2, $2, $3, $4, $5, $6, $7, TRUE, TRUE, TRUE, '{}', TRUE, FALSE, $8)",
            &[
                &id,
                &now,
                &workspace,
                &project,
                &entity_identifier,
                &entity_name,
                &anchor,
                &deleted.then_some(now),
            ],
        )
        .await
        .expect("insert a board");
    fixture.boards.push(id);
    id
}

fn is_object_not_found(error: &ViewError) -> bool {
    matches!(error, ViewError::Api(ApiError::ObjectDoesNotExist))
}

#[tokio::test]
async fn board_resolution_matches_the_orm() {
    let Some(client) = client().await else {
        return;
    };
    let mut fixture = Fixture::default();

    let owner = insert_user(&client, &mut fixture).await;
    let other = insert_user(&client, &mut fixture).await;
    let workspace = insert_workspace(&client, &mut fixture, owner).await;
    let project = insert_project(&client, &mut fixture, workspace, false).await;
    let archived_project = insert_project(&client, &mut fixture, workspace, true).await;
    let triage = insert_state(&client, &mut fixture, workspace, project, "triage").await;

    // Two boards per anchor shape: a project board and a page board.
    let anchor = format!("pubp0-{}", Uuid::new_v4().simple());
    let project_board = insert_board(
        &client,
        &mut fixture,
        workspace,
        Some(project),
        Some(project),
        "project",
        &anchor,
        false,
    )
    .await;
    let page_anchor = format!("pubp0-{}", Uuid::new_v4().simple());
    insert_board(
        &client,
        &mut fixture,
        workspace,
        Some(project),
        Some(Uuid::new_v4()),
        "page",
        &page_anchor,
        false,
    )
    .await;
    let deleted_anchor = format!("pubp0-{}", Uuid::new_v4().simple());
    insert_board(
        &client,
        &mut fixture,
        workspace,
        Some(project),
        Some(project),
        "project",
        &deleted_anchor,
        true,
    )
    .await;

    // --- the lookups ------------------------------------------------------
    let board = board::published_board(&client, &anchor)
        .await
        .expect("published_board")
        .expect("the project board");
    assert_eq!(board.id, project_board);
    assert_eq!(board.workspace_id, workspace);
    assert_eq!(board.project_id, Some(project));
    assert_eq!(board.entity_identifier, Some(project));
    assert_eq!(board.anchor, anchor);
    assert!(board.is_comments_enabled && board.is_reactions_enabled && board.is_votes_enabled);
    assert!(board.is_activity_enabled && !board.is_disabled);
    assert_eq!(board.deleted_at, None);

    // `.filter(anchor, entity_name="project")` skips a page board …
    assert!(
        board::published_board(&client, &page_anchor)
            .await
            .expect("published_board")
            .is_none()
    );
    // … while `.filter(anchor)` finds it.
    let page = board::board_by_anchor(&client, &page_anchor)
        .await
        .expect("board_by_anchor")
        .expect("the page board");
    assert_eq!(page.entity_name.as_deref(), Some("page"));

    // A soft-deleted board is a miss for both: `deleted_at IS NULL`.
    assert!(
        board::published_board(&client, &deleted_anchor)
            .await
            .expect("published_board")
            .is_none()
    );
    assert!(
        board::board_by_anchor(&client, &deleted_anchor)
            .await
            .expect("board_by_anchor")
            .is_none()
    );

    // `.get()` on a miss is `ObjectDoesNotExist`, not the view's own 404.
    let missing = format!("pubp0-{}", Uuid::new_v4().simple());
    assert!(is_object_not_found(
        &board::require_published_board(&client, &missing)
            .await
            .expect_err("an unknown anchor")
    ));
    assert!(is_object_not_found(
        &board::require_board(&client, &missing)
            .await
            .expect_err("an unknown anchor")
    ));
    // A page board answers `require_board` but not `require_published_board`.
    assert_eq!(
        board::require_board(&client, &page_anchor)
            .await
            .expect("require_board")
            .entity_name
            .as_deref(),
        Some("page")
    );
    assert!(is_object_not_found(
        &board::require_published_board(&client, &page_anchor)
            .await
            .expect_err("a page board is not a project board")
    ));

    // --- the write predicates --------------------------------------------
    let visible = insert_issue(
        &client,
        &mut fixture,
        workspace,
        project,
        None,
        false,
        false,
        false,
    )
    .await;
    assert!(
        board::issue_in_board_scope(&client, visible, &board)
            .await
            .expect("issue_in_board_scope")
    );
    // Issues the manager excludes: draft, archived, triage state, a
    // soft-deleted row, another project, an archived project.
    for (state, draft, archived, deleted, project_id) in [
        (None, true, false, false, project),
        (None, false, true, false, project),
        (Some(triage), false, false, false, project),
        (None, false, false, true, project),
        (None, false, false, false, archived_project),
    ] {
        let hidden = insert_issue(
            &client,
            &mut fixture,
            workspace,
            project_id,
            state,
            draft,
            archived,
            deleted,
        )
        .await;
        assert!(
            !board::issue_in_board_scope(&client, hidden, &board)
                .await
                .expect("issue_in_board_scope"),
            "an issue the manager excludes must not be writable"
        );
    }

    let external = insert_comment(
        &client,
        &mut fixture,
        workspace,
        project,
        visible,
        owner,
        "EXTERNAL",
        false,
    )
    .await;
    assert!(
        board::comment_in_board_scope(&client, external, &board)
            .await
            .expect("comment_in_board_scope")
    );
    assert!(
        board::is_own_board_comment(&client, external, &board, owner)
            .await
            .expect("is_own_board_comment")
    );
    assert!(
        !board::is_own_board_comment(&client, external, &board, other)
            .await
            .expect("is_own_board_comment")
    );
    let internal = insert_comment(
        &client,
        &mut fixture,
        workspace,
        project,
        visible,
        owner,
        "INTERNAL",
        false,
    )
    .await;
    assert!(
        !board::comment_in_board_scope(&client, internal, &board)
            .await
            .expect("comment_in_board_scope")
    );
    assert!(
        !board::is_own_board_comment(&client, internal, &board, owner)
            .await
            .expect("is_own_board_comment")
    );
    let deleted_comment = insert_comment(
        &client,
        &mut fixture,
        workspace,
        project,
        visible,
        owner,
        "EXTERNAL",
        true,
    )
    .await;
    assert!(
        !board::comment_in_board_scope(&client, deleted_comment, &board)
            .await
            .expect("comment_in_board_scope")
    );

    fixture.teardown(&client).await;
}
