// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/bgtasks/workspace_seed_task.py:448-517` — `workspace_seed`.
//!
//! One task call turns a new workspace into a demo workspace: a bot user, the
//! bot's workspace membership, one project copied from the seed JSON, its
//! states, labels, cycles, modules, issues (with sequences, activities, label,
//! cycle and module links), one issue view and two pages. The seeds are the
//! checked-in files under `apps/api/plane/seeds/data`; `crates/plane-tasks/
//! data/seeds` is a byte copy (the corpus test below pins the copy).
//!
//! # What matters for parity
//!
//! * `workspace_seed` is the one task that **re-raises**: its `except` logs and
//!   `raise e`s (`workspace_seed_task.py:513-516`), so a missing workspace is a
//!   failed job, not a logged no-op.
//! * `BaseModel.save` overrides `created_by`/`updated_by` with the request user
//!   unless `disable_auto_set_user=True`, and a task has no request user
//!   (`plane/db/models/base.py:23-44`). Every `objects.create(...)` in the task
//!   therefore stores **null** audit columns; the rows it saves with
//!   `disable_auto_set_user=True` store the bot.
//! * `Issue.save` **ignores** the seed's `sequence_id` and `sort_order`: the
//!   sequence is `max(sequence) + 1` over the project and the sort order keeps
//!   the seed only for the first issue in a state (`issue.py:180-214`). It also
//!   computes `description_stripped` with `strip_tags` and sets `completed_at`
//!   when the state group is `completed` (`issue.py:240-255`), and it inserts
//!   **one** `IssueSequence` itself; the task inserts a second one with the
//!   field default `sequence=1` (`issue.py:557-571`).
//! * `State.save` derives the slug with `slugify` and, after the first state,
//!   sets `sequence = max(sequence) + 15000`, overriding the seed
//!   (`state.py:117-126`). `Cycle.save`/`Module.save` set `sort_order` to
//!   `min - 10000` after the first seed (`cycle.py:88-97`,
//!   `module.py:115-124`).
//! * `Page.save` recomputes `description_stripped` from the HTML
//!   (`page.py:70-77`); the seed's own value is ignored. The seed's
//!   `description` key is *not* `description_json`, so the JSON stays `{}`.
//! * Timestamps are `timezone.now()`; the `difftest task` diff normalises them.
//!   The bot's password is `make_password(uuid4().hex)` (`make_password` with
//!   the default 1,000,000-iteration PBKDF2 hasher), and its e-mail is
//!   `bot_user_<workspace id>@<WEB_URL host>`
//!   (`workspace_seed_task.py:479-490`).
//! * The task is not transactional: a later failure leaves the earlier rows.
//!
//! Ownership: the only producer is the workspace create POST, a Rust route
//! (`plane-http-app/src/workspaces/write.rs:511-515`), so the task moves with
//! this handler.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Days, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskContext, kwargs, uuid_value};

/// The Celery task name (`plane/bgtasks/workspace_seed_task.py:448`).
pub const TASK: &str = "plane.bgtasks.workspace_seed_task.workspace_seed";

/// Register the handler.
pub fn register(registry: &mut TaskRegistry, _context: Arc<TaskContext>) {
    registry.register(TASK, handle);
}

async fn handle(job: JobContext) -> Result<(), HandlerError> {
    // The producer calls `workspace_seed.delay(workspace_id)` positionally
    // (`plane/app/views/workspace/base.py:138`), so the real job carries
    // `args[0]`; `difftest task` sends the same value as a keyword argument.
    let workspace_id = kwargs(&job.payload)
        .and_then(|kw| uuid_value(kw.get("workspace_id")))
        .or_else(|| crate::args(&job.payload).and_then(|args| uuid_value(args.first())));
    let Some(workspace_id) = workspace_id else {
        // The payload's `TypeError`/`KeyError` is inside the task's `try`; the
        // task logs it and re-raises.
        return Err(HandlerError::fatal(
            "workspace_seed payload has no workspace_id",
        ));
    };
    run(&job.client, workspace_id, Utc::now())
        .await
        .map_err(HandlerError::fatal)
}

/// A task failure, rendered as the outer `except`'s log line.
type Failure = String;

struct Workspace {
    id: Uuid,
    name: String,
    timezone: String,
}

// The seeds are embedded: the worker image has no `apps/api` directory, and a
// missing seed file must not make the task silently skip work.
const PROJECTS: &str = include_str!("../data/seeds/projects.json");
const STATES: &str = include_str!("../data/seeds/states.json");
const LABELS: &str = include_str!("../data/seeds/labels.json");
const CYCLES: &str = include_str!("../data/seeds/cycles.json");
const MODULES: &str = include_str!("../data/seeds/modules.json");
const ISSUES: &str = include_str!("../data/seeds/issues.json");
const VIEWS: &str = include_str!("../data/seeds/views.json");
const PAGES: &str = include_str!("../data/seeds/pages.json");

#[derive(Debug, Deserialize)]
struct ProjectSeed {
    id: i64,
    description: String,
    network: i16,
    cover_image: String,
    logo_props: Value,
}

#[derive(Debug, Deserialize)]
struct StateSeed {
    id: i64,
    name: String,
    color: String,
    sequence: f64,
    group: String,
    default: bool,
    project_id: i64,
}

#[derive(Debug, Deserialize)]
struct LabelSeed {
    id: i64,
    name: String,
    color: String,
    sort_order: f64,
    project_id: i64,
}

#[derive(Debug, Deserialize)]
struct CycleSeed {
    id: i64,
    name: String,
    project_id: i64,
    sort_order: f64,
    timezone: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct ModuleSeed {
    id: i64,
    name: String,
    project_id: i64,
    sort_order: f64,
    status: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct IssueSeed {
    // `id` and `sequence_id` are in the seed but `Issue.save` ignores both on
    // create: the id is a fresh uuid and the sequence is `max(sequence) + 1`.
    name: String,
    description_html: String,
    priority: String,
    #[serde(default)]
    sort_order: f64,
    state_id: i64,
    project_id: i64,
    labels: Vec<i64>,
    cycle_id: Option<i64>,
    module_ids: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct ViewSeed {
    name: String,
    description: String,
    access: i16,
    filters: Value,
    project_id: i64,
    display_filters: Value,
    display_properties: Value,
    sort_order: f64,
    rich_filters: Value,
}

#[derive(Debug, Deserialize)]
struct PageSeed {
    name: String,
    project_id: i64,
    description_html: String,
    access: i16,
    #[serde(rename = "type")]
    kind: String,
}

struct Seeds {
    projects: Vec<ProjectSeed>,
    states: Vec<StateSeed>,
    labels: Vec<LabelSeed>,
    cycles: Vec<CycleSeed>,
    modules: Vec<ModuleSeed>,
    issues: Vec<IssueSeed>,
    views: Vec<ViewSeed>,
    pages: Vec<PageSeed>,
}

fn seeds() -> &'static Seeds {
    static SEEDS: LazyLock<Seeds> = LazyLock::new(|| Seeds {
        projects: serde_json::from_str(PROJECTS).expect("projects.json"),
        states: serde_json::from_str(STATES).expect("states.json"),
        labels: serde_json::from_str(LABELS).expect("labels.json"),
        cycles: serde_json::from_str(CYCLES).expect("cycles.json"),
        modules: serde_json::from_str(MODULES).expect("modules.json"),
        issues: serde_json::from_str(ISSUES).expect("issues.json"),
        views: serde_json::from_str(VIEWS).expect("views.json"),
        pages: serde_json::from_str(PAGES).expect("pages.json"),
    });
    &SEEDS
}

/// `get_default_props`/`get_issue_props` (`plane/db/models/workspace.py:190-215`):
/// the JSON a `WorkspaceMember`/`ProjectMember` gets when the caller does not
/// pass one.
const DEFAULT_PROPS: &str = r#"{"filters": {"state": null, "labels": null, "priority": null, "assignees": null, "created_by": null, "start_date": null, "subscriber": null, "state_group": null, "target_date": null}, "display_filters": {"type": null, "layout": "list", "group_by": null, "order_by": "-created_at", "sub_issue": true, "show_empty_groups": true, "calendar_date_range": ""}, "display_properties": {"key": true, "link": true, "state": true, "labels": true, "assignee": true, "due_date": true, "estimate": true, "priority": true, "created_on": true, "start_date": true, "updated_on": true, "sub_issue_count": true, "attachment_count": true}}"#;

const ISSUE_PROPS: &str =
    r#"{"created": true, "assigned": true, "all_issues": true, "subscribed": true}"#;

/// `ProjectMember.preferences` default (`plane/db/models/project.py`).
const MEMBER_PREFERENCES: &str = r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#;

/// `ProjectUserProperty.filters` default.
const USER_PROPERTY_FILTERS: &str = r#"{"state": null, "labels": null, "priority": null, "assignees": null, "created_by": null, "start_date": null, "subscriber": null, "state_group": null, "target_date": null}"#;

/// The literal `display_filters`/`display_properties` the task passes
/// (`workspace_seed_task.py:146-190`).
const USER_PROPERTY_DISPLAY_FILTERS: &str = r#"{"layout": "list", "calendar": {"layout": "month", "show_weekends": false}, "group_by": "state", "order_by": "sort_order", "sub_issue": true, "sub_group_by": null, "show_empty_groups": true}"#;

const USER_PROPERTY_DISPLAY_PROPERTIES: &str = r#"{"key": true, "link": true, "cycle": false, "state": true, "labels": false, "modules": false, "assignee": true, "due_date": false, "estimate": true, "priority": true, "created_on": true, "issue_type": true, "start_date": false, "updated_on": true, "customer_count": true, "sub_issue_count": false, "attachment_count": false, "customer_request_count": true}"#;

/// `slugify` (`django/utils/text.py`): lower-case, drop anything that is not a
/// letter, digit, underscore, hyphen or space, turn spaces into hyphens and
/// strip hyphens/underscores from the ends.
fn slugify(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            out.extend(ch.to_lowercase());
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    out.trim_matches(['-', '_']).to_string()
}

/// Python `''.join(ch for ch in name if ch.isalnum())[:5]`, upper-cased by
/// `Project.save` (`project.py:196`).
fn project_identifier(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_alphanumeric())
        .take(5)
        .collect::<String>()
        .to_uppercase()
}

/// A JSON literal as the `jsonb` value a `ToSql` parameter needs (a `&str`
/// does not implement `ToSql for Jsonb`).
macro_rules! json_const {
    ($name:ident, $text:expr) => {
        fn $name() -> &'static Value {
            static VALUE: LazyLock<Value> = LazyLock::new(|| {
                serde_json::from_str($text).expect(concat!(stringify!($name), " is valid JSON"))
            });
            &VALUE
        }
    };
}

json_const!(default_props, DEFAULT_PROPS);
json_const!(issue_props, ISSUE_PROPS);
json_const!(member_view_props, PROJECT_MEMBER_VIEW_PROPS);
json_const!(member_preferences, MEMBER_PREFERENCES);
json_const!(user_property_filters, USER_PROPERTY_FILTERS);
json_const!(user_property_display_filters, USER_PROPERTY_DISPLAY_FILTERS);
json_const!(
    user_property_display_properties,
    USER_PROPERTY_DISPLAY_PROPERTIES
);

/// `urlparse(settings.WEB_URL or 'https://plane.so').hostname or 'plane.so'`.
fn web_host(web_url: Option<&str>) -> String {
    let Some(url) = web_url else {
        return "plane.so".to_string();
    };
    let Some(rest) = url
        .split_once("://")
        .map_or(Some(url), |(_, rest)| Some(rest))
    else {
        return "plane.so".to_string();
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split_once(':').map_or(host, |(host, _)| host);
    if host.is_empty() {
        "plane.so".to_string()
    } else {
        host.to_string()
    }
}

async fn run(client: &Client, workspace_id: Uuid, now: DateTime<Utc>) -> Result<(), Failure> {
    // `Workspace.objects.get(id=workspace_id)`: a missing row raises and the
    // task re-raises.
    let row = client
        .query_opt(
            "SELECT name, timezone FROM workspaces WHERE id = $1",
            &[&workspace_id],
        )
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Workspace matching query does not exist.".to_string())?;
    let name: String = row.get(0);
    let timezone: String = row.get(1);
    let workspace = Workspace {
        id: workspace_id,
        name,
        timezone,
    };

    let bot = create_bot_user(client, &workspace, now).await?;
    create_bot_membership(client, &workspace, bot, now).await?;

    let projects = create_projects(client, &workspace, bot, now).await?;
    let states = create_states(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("states: {error}"))?;
    let labels = create_labels(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("labels: {error}"))?;
    let cycles = create_cycles(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("cycles: {error}"))?;
    let modules = create_modules(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("modules: {error}"))?;
    create_issues(
        client, &workspace, bot, &projects, &states, &labels, &cycles, &modules, now,
    )
    .await
    .map_err(|error| format!("issues: {error}"))?;
    create_views(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("views: {error}"))?;
    create_pages(client, &workspace, bot, &projects, now)
        .await
        .map_err(|error| format!("pages: {error}"))?;
    Ok(())
}

async fn create_bot_user(
    client: &Client,
    workspace: &Workspace,
    now: DateTime<Utc>,
) -> Result<Uuid, Failure> {
    let id = Uuid::new_v4();
    let username = format!("bot_user_{}", workspace.id);
    let web_url = web_host_env();
    let email = format!("bot_user_{}@{}", workspace.id, web_host(web_url.as_deref()));
    // `make_password(uuid.uuid4().hex)`: Django's default hasher and salt.
    let password = plane_auth::make_password(Some(&Uuid::new_v4().simple().to_string()));
    client
        .execute(
            "INSERT INTO users (password, id, username, email, first_name, last_name, \
             avatar, date_joined, created_at, updated_at, last_location, created_location, \
             is_superuser, is_managed, is_password_expired, is_active, is_staff, \
             is_email_verified, is_password_autoset, token, user_timezone, last_active, \
             last_login_ip, last_logout_ip, last_login_medium, last_login_uagent, \
             is_bot, cover_image, display_name, bot_type, is_email_valid, \
             is_password_reset_required) \
             VALUES ($1, $2, $3, $4, 'Plane', '', '', $5, $5, $5, '', '', \
             false, false, false, true, false, false, true, '', 'UTC', $5, \
             '', '', 'email', '', true, NULL, 'Plane', 'WORKSPACE_SEED', false, false)",
            &[&password, &id, &username, &email, &now],
        )
        .await
        .map_err(|error| format!("insert bot user: {error}"))?;
    Ok(id)
}

/// `WEB_URL` from the worker's process environment, as `Settings` parses it.
fn web_host_env() -> Option<String> {
    std::env::var("WEB_URL").ok()
}

async fn create_bot_membership(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    client
        .execute(
            "INSERT INTO workspace_members (created_at, updated_at, id, role, member_id, \
             workspace_id, company_role, view_props, default_props, issue_props, is_active, \
             explored_features, getting_started_checklist, tips) \
             VALUES ($1, $1, $2, 20, $3, $4, '', $5::jsonb, $5::jsonb, $6::jsonb, true, \
             '{}'::jsonb, '{}'::jsonb, '{}'::jsonb)",
            &[
                &now,
                &Uuid::new_v4(),
                &bot,
                &workspace.id,
                default_props(),
                issue_props(),
            ],
        )
        .await
        .map_err(|error| format!("insert bot membership: {error}"))?;
    Ok(())
}

/// One id per seed project id.
async fn create_projects(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    now: DateTime<Utc>,
) -> Result<HashMap<i64, Uuid>, Failure> {
    let mut map = HashMap::new();
    for seed in &seeds().projects {
        let id = Uuid::new_v4();
        let identifier = project_identifier(&workspace.name);
        // `Project(**seed, workspace=..., name=workspace.name, identifier=...,
        // created_by_id=bot, cycle_view/module_view/issue_views_view=True)`.
        client
            .execute(
                "INSERT INTO projects (created_at, updated_at, id, name, description, \
                 description_text, description_html, network, identifier, created_by_id, \
                 workspace_id, cycle_view, module_view, cover_image, issue_views_view, \
                 page_view, intake_view, archive_in, close_in, logo_props, \
                 is_time_tracking_enabled, is_issue_type_enabled, guest_view_all_features, \
                 timezone) \
                 VALUES ($1, $1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, true, true, $9, \
                 true, true, false, 0, 0, $10::jsonb, false, false, false, $11)",
                &[
                    &now,
                    &id,
                    &workspace.name.as_str(),
                    &seed.description,
                    &seed.network,
                    &identifier,
                    &bot,
                    &workspace.id,
                    &seed.cover_image,
                    &seed.logo_props,
                    &workspace.timezone.as_str(),
                ],
            )
            .await
            .map_err(|error| format!("insert project: {error}"))?;

        // `WorkspaceMember.objects.filter(workspace=workspace)`.
        let members = client
            .query(
                "SELECT member_id, role FROM workspace_members WHERE workspace_id = $1",
                &[&workspace.id],
            )
            .await
            .map_err(|error| error.to_string())?;
        for member in &members {
            let member_id: Uuid = member.get(0);
            let role: i16 = member.get(1);
            client
                .execute(
                    "INSERT INTO project_members (created_at, updated_at, id, comment, role, \
                     created_by_id, member_id, project_id, workspace_id, view_props, \
                     default_props, sort_order, preferences, is_active) \
                     VALUES ($1, $1, $2, NULL, $3, $4, $5, $6, $7, $8::jsonb, $8::jsonb, \
                     65535.0, $9::jsonb, true)",
                    &[
                        &now,
                        &Uuid::new_v4(),
                        &role,
                        &bot,
                        &member_id,
                        &id,
                        &workspace.id,
                        member_view_props(),
                        member_preferences(),
                    ],
                )
                .await
                .map_err(|error| error.to_string())?;
            client
                .execute(
                    "INSERT INTO project_user_properties (created_at, updated_at, id, \
                     display_properties, created_by_id, project_id, user_id, workspace_id, \
                     display_filters, filters, rich_filters, preferences, sort_order) \
                     VALUES ($1, $1, $2, $3::jsonb, $4, $5, $6, $7, $8::jsonb, $9::jsonb, \
                     '{}'::jsonb, $10::jsonb, 65535.0)",
                    &[
                        &now,
                        &Uuid::new_v4(),
                        user_property_display_properties(),
                        &bot,
                        &id,
                        &member_id,
                        &workspace.id,
                        user_property_display_filters(),
                        user_property_filters(),
                        member_preferences(),
                    ],
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        map.insert(seed.id, id);
    }
    Ok(map)
}

/// `ProjectMember.view_props` default (the workspace member's `display_props`
/// minus `display_properties`).
const PROJECT_MEMBER_VIEW_PROPS: &str = r#"{"filters": {"state": null, "labels": null, "priority": null, "assignees": null, "created_by": null, "start_date": null, "subscriber": null, "state_group": null, "target_date": null}, "display_filters": {"type": null, "layout": "list", "group_by": null, "order_by": "-created_at", "sub_issue": true, "show_empty_groups": true, "calendar_date_range": ""}}"#;

async fn create_states(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<HashMap<i64, Uuid>, Failure> {
    let mut map = HashMap::new();
    for seed in &seeds().states {
        let Some(project) = projects.get(&seed.project_id) else {
            continue;
        };
        // `State.save` recomputes `sequence` after the first state.
        let largest: Option<f64> = client
            .query_one(
                "SELECT MAX(sequence) FROM states WHERE project_id = $1",
                &[project],
            )
            .await
            .map_err(|error| error.to_string())?
            .get(0);
        let sequence = largest.map_or(seed.sequence, |largest| largest + 15000.0);
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO states (created_at, updated_at, id, name, description, color, \
                 slug, created_by_id, project_id, workspace_id, sequence, \"group\", \
                 \"default\", is_triage) \
                 VALUES ($1, $1, $2, $3, '', $4, $5, $6, $7, $8, $9, $10, $11, false)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &seed.color,
                    &slugify(&seed.name),
                    &bot,
                    project,
                    &workspace.id,
                    &sequence,
                    &seed.group,
                    &seed.default,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        map.insert(seed.id, id);
    }
    Ok(map)
}

async fn create_labels(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<HashMap<i64, Uuid>, Failure> {
    let mut map = HashMap::new();
    for seed in &seeds().labels {
        let Some(project) = projects.get(&seed.project_id) else {
            continue;
        };
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO labels (created_at, updated_at, id, name, description, \
                 created_by_id, project_id, workspace_id, color, sort_order) \
                 VALUES ($1, $1, $2, $3, '', $4, $5, $6, $7, $8)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &bot,
                    project,
                    &workspace.id,
                    &seed.color,
                    &seed.sort_order,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        map.insert(seed.id, id);
    }
    Ok(map)
}

async fn create_cycles(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<HashMap<i64, Uuid>, Failure> {
    let mut map = HashMap::new();
    for seed in &seeds().cycles {
        let Some(project) = projects.get(&seed.project_id) else {
            continue;
        };
        let (start, end) = match seed.kind.as_str() {
            "CURRENT" => (now, now + chrono::Duration::days(14)),
            _ => {
                let last: Option<DateTime<Utc>> = client
                    .query_one(
                        "SELECT end_date FROM cycles WHERE project_id = $1 \
                         ORDER BY end_date DESC NULLS LAST LIMIT 1",
                        &[project],
                    )
                    .await
                    .map_err(|error| error.to_string())?
                    .get(0);
                let start = last.map_or(now + chrono::Duration::days(14), |last| {
                    last + chrono::Duration::days(1)
                });
                (start, start + chrono::Duration::days(14))
            }
        };
        let smallest: Option<f64> = client
            .query_one(
                "SELECT MIN(sort_order) FROM cycles WHERE project_id = $1",
                &[project],
            )
            .await
            .map_err(|error| error.to_string())?
            .get(0);
        let sort_order = smallest.map_or(seed.sort_order, |smallest| smallest - 10000.0);
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO cycles (created_at, updated_at, id, name, description, \
                 start_date, end_date, created_by_id, owned_by_id, project_id, workspace_id, \
                 view_props, sort_order, progress_snapshot, logo_props, timezone, version) \
                 VALUES ($1, $1, $2, $3, '', $4, $5, $6, $6, $7, $8, '{}'::jsonb, $9, \
                 '{}'::jsonb, '{}'::jsonb, $10, 1)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &start,
                    &end,
                    &bot,
                    project,
                    &workspace.id,
                    &sort_order,
                    &seed.timezone,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        map.insert(seed.id, id);
    }
    Ok(map)
}

async fn create_modules(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<HashMap<i64, Uuid>, Failure> {
    let today: NaiveDate = now.date_naive();
    let mut map = HashMap::new();
    for (index, seed) in seeds().modules.iter().enumerate() {
        let Some(project) = projects.get(&seed.project_id) else {
            continue;
        };
        let start = today + Days::new((index as u64) * 2);
        let target = start + Days::new(14);
        let smallest: Option<f64> = client
            .query_one(
                "SELECT MIN(sort_order) FROM modules WHERE project_id = $1",
                &[project],
            )
            .await
            .map_err(|error| error.to_string())?
            .get(0);
        let sort_order = smallest.map_or(seed.sort_order, |smallest| smallest - 10000.0);
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO modules (created_at, updated_at, id, name, description, \
                 description_text, description_html, start_date, target_date, status, \
                 created_by_id, project_id, workspace_id, view_props, sort_order, \
                 logo_props) \
                 VALUES ($1, $1, $2, $3, $4, NULL, NULL, $5, $6, $7, $8, $9, $10, \
                 '{}'::jsonb, $11, '{}'::jsonb)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &seed.description,
                    &start,
                    &target,
                    &seed.status,
                    &bot,
                    project,
                    &workspace.id,
                    &sort_order,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        map.insert(seed.id, id);
    }
    Ok(map)
}

#[allow(clippy::too_many_arguments)]
async fn create_issues(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    states: &HashMap<i64, Uuid>,
    labels: &HashMap<i64, Uuid>,
    cycles: &HashMap<i64, Uuid>,
    modules: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    for seed in &seeds().issues {
        let (Some(project), Some(state)) =
            (projects.get(&seed.project_id), states.get(&seed.state_id))
        else {
            continue;
        };
        // `Issue.save`'s create branch (`issue.py:184-214`).
        let sequence: i64 = client
            .query_one(
                "SELECT COALESCE(MAX(sequence), 0) FROM issue_sequences WHERE project_id = $1",
                &[project],
            )
            .await
            .map_err(|error| error.to_string())?
            .get::<_, i64>(0)
            + 1;
        let stripped: Option<String> = if seed.description_html.is_empty() {
            None
        } else {
            Some(crate::bs4::strip_tags(&seed.description_html))
        };
        let largest: Option<f64> = client
            .query_one(
                "SELECT MAX(sort_order) FROM issues WHERE project_id = $1 AND state_id = $2",
                &[project, state],
            )
            .await
            .map_err(|error| error.to_string())?
            .get(0);
        let sort_order = largest.map_or(seed.sort_order, |largest| largest + 10000.0);
        let group: String = client
            .query_one("SELECT \"group\" FROM states WHERE id = $1", &[state])
            .await
            .map_err(|error| error.to_string())?
            .get(0);
        let completed_at = if group == "completed" {
            Some(now)
        } else {
            None
        };
        let id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO issues (created_at, updated_at, id, name, description_json, \
                 priority, sequence_id, created_by_id, project_id, state_id, workspace_id, \
                 description_html, description_stripped, completed_at, sort_order, is_draft) \
                 VALUES ($1, $1, $2, $3, '{}'::jsonb, $4, $5, $6, $7, $8, $9, $10, $11, \
                 $12, $13, false)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &seed.priority,
                    &(sequence as i32),
                    &bot,
                    project,
                    state,
                    &workspace.id,
                    &seed.description_html,
                    &stripped,
                    &completed_at,
                    &sort_order,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        // The sequence `Issue.save` inserts itself (`sequence=self.sequence_id`),
        // then the task's own with the field default 1 (`issue.py:214`,
        // `workspace_seed_task.py:279-284`). Both have null audit columns.
        client
            .execute(
                "INSERT INTO issue_sequences (created_at, updated_at, id, sequence, deleted, \
                 issue_id, project_id, workspace_id) \
                 VALUES ($1, $1, $2, $3, false, $4, $5, $6)",
                &[
                    &now,
                    &Uuid::new_v4(),
                    &sequence,
                    &id,
                    project,
                    &workspace.id,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        client
            .execute(
                "INSERT INTO issue_sequences (created_at, updated_at, id, sequence, deleted, \
                 issue_id, project_id, workspace_id) \
                 VALUES ($1, $1, $2, 1, false, $3, $4, $5)",
                &[&now, &Uuid::new_v4(), &id, project, &workspace.id],
            )
            .await
            .map_err(|error| error.to_string())?;
        client
            .execute(
                "INSERT INTO issue_activities (created_at, updated_at, id, verb, comment, \
                 attachments, issue_id, project_id, workspace_id, actor_id, epoch) \
                 VALUES ($1, $1, $2, 'created', 'created the issue', '{}'::varchar[], $3, \
                 $4, $5, $6, $7)",
                &[
                    &now,
                    &Uuid::new_v4(),
                    &id,
                    project,
                    &workspace.id,
                    &bot,
                    &(now.timestamp_millis() as f64 / 1000.0),
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        for label in &seed.labels {
            let Some(label) = labels.get(label) else {
                continue;
            };
            client
                .execute(
                    "INSERT INTO issue_labels (created_at, updated_at, id, issue_id, \
                     label_id, project_id, workspace_id) \
                     VALUES ($1, $1, $2, $3, $4, $5, $6)",
                    &[&now, &Uuid::new_v4(), &id, label, project, &workspace.id],
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        if let Some(cycle) = seed.cycle_id.and_then(|cycle| cycles.get(&cycle)) {
            client
                .execute(
                    "INSERT INTO cycle_issues (created_at, updated_at, id, cycle_id, issue_id, \
                     project_id, workspace_id) VALUES ($1, $1, $2, $3, $4, $5, $6)",
                    &[&now, &Uuid::new_v4(), cycle, &id, project, &workspace.id],
                )
                .await
                .map_err(|error| error.to_string())?;
        }
        for module in &seed.module_ids {
            let Some(module) = modules.get(module) else {
                continue;
            };
            client
                .execute(
                    "INSERT INTO module_issues (created_at, updated_at, id, issue_id, \
                     module_id, project_id, workspace_id) \
                     VALUES ($1, $1, $2, $3, $4, $5, $6)",
                    &[&now, &Uuid::new_v4(), &id, module, project, &workspace.id],
                )
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

async fn create_views(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    for seed in &seeds().views {
        let Some(project) = projects.get(&seed.project_id) else {
            continue;
        };
        client
            .execute(
                "INSERT INTO issue_views (created_at, updated_at, id, name, description, \
                 query, access, filters, created_by_id, project_id, workspace_id, \
                 display_filters, display_properties, sort_order, logo_props, is_locked, \
                 owned_by_id, rich_filters) \
                 VALUES ($1, $1, $2, $3, $4, '{}'::jsonb, $5, $6::jsonb, $7, $8, $9, \
                 $10::jsonb, $11::jsonb, $12, '{}'::jsonb, false, $7, $13::jsonb)",
                &[
                    &now,
                    &Uuid::new_v4(),
                    &seed.name,
                    &seed.description,
                    &seed.access,
                    &seed.filters,
                    &bot,
                    project,
                    &workspace.id,
                    &seed.display_filters,
                    &seed.display_properties,
                    &seed.sort_order,
                    &seed.rich_filters,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

async fn create_pages(
    client: &Client,
    workspace: &Workspace,
    bot: Uuid,
    projects: &HashMap<i64, Uuid>,
    now: DateTime<Utc>,
) -> Result<(), Failure> {
    for seed in &seeds().pages {
        let id = Uuid::new_v4();
        let stripped: Option<String> = if seed.description_html.is_empty() {
            None
        } else {
            Some(crate::bs4::strip_tags(&seed.description_html))
        };
        client
            .execute(
                "INSERT INTO pages (created_at, updated_at, id, name, description_json, \
                 description_html, description_stripped, access, created_by_id, owned_by_id, \
                 updated_by_id, workspace_id, color, is_locked, view_props, logo_props, \
                 is_global, sort_order) \
                 VALUES ($1, $1, $2, $3, '{}'::jsonb, $4, $5, $6, $7, $7, $7, $8, '', \
                 false, '{\"full_width\": false}'::jsonb, '{}'::jsonb, false, 65535.0)",
                &[
                    &now,
                    &id,
                    &seed.name,
                    &seed.description_html,
                    &stripped,
                    &seed.access,
                    &bot,
                    &workspace.id,
                ],
            )
            .await
            .map_err(|error| error.to_string())?;
        if seed.kind == "PROJECT"
            && let Some(project) = projects.get(&seed.project_id)
        {
            client
                .execute(
                    "INSERT INTO project_pages (created_at, updated_at, id, created_by_id, \
                     page_id, project_id, updated_by_id, workspace_id) \
                     VALUES ($1, $1, $2, $3, $4, $5, $3, $6)",
                    &[&now, &Uuid::new_v4(), &bot, &id, project, &workspace.id],
                )
                .await
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_seed_file_parses() {
        let seeds = seeds();
        assert_eq!(seeds.projects.len(), 1);
        assert_eq!(seeds.states.len(), 5);
        assert_eq!(seeds.labels.len(), 2);
        assert_eq!(seeds.cycles.len(), 2);
        assert_eq!(seeds.modules.len(), 3);
        assert_eq!(seeds.issues.len(), 7);
        assert_eq!(seeds.views.len(), 1);
        assert_eq!(seeds.pages.len(), 2);
    }

    #[test]
    fn the_project_identifier_is_five_alphanumerics_upper_cased() {
        assert_eq!(project_identifier("P8C Seed Workspace"), "P8CSE");
        assert_eq!(project_identifier("ab"), "AB");
    }

    #[test]
    fn state_slugs_follow_django_slugify() {
        assert_eq!(slugify("In Progress"), "in-progress");
        assert_eq!(slugify("Backlog"), "backlog");
        assert_eq!(slugify("  Done  "), "done");
    }

    #[test]
    fn the_bot_email_host_is_the_web_url_host() {
        assert_eq!(web_host(Some("http://127.0.0.1:18080")), "127.0.0.1");
        assert_eq!(web_host(Some("https://app.plane.so/")), "app.plane.so");
        assert_eq!(web_host(None), "plane.so");
        assert_eq!(web_host(Some("http://user@host:1/x")), "host");
    }

    #[test]
    fn the_embedded_seeds_are_the_django_files() {
        // `apps/api` is the specification: the copies here must not drift.
        // The test reads the Django tree only when it is present (CI checks
        // out the whole repository; a distroless worker does not run tests).
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..");
        if !root.join("apps/api/plane/seeds/data").is_dir() {
            return;
        }
        for (name, embedded) in [
            ("projects.json", PROJECTS),
            ("states.json", STATES),
            ("labels.json", LABELS),
            ("cycles.json", CYCLES),
            ("modules.json", MODULES),
            ("issues.json", ISSUES),
            ("views.json", VIEWS),
            ("pages.json", PAGES),
        ] {
            let path = root.join("apps/api/plane/seeds/data").join(name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            assert_eq!(text, embedded, "{name} differs from {}", path.display());
        }
    }

    #[test]
    fn the_payload_is_read_positionally_and_by_keyword() {
        let id = "a0000000-0000-4000-8000-000000000010";
        let positional = serde_json::json!({"args": [id], "kwargs": {}});
        let keyword = serde_json::json!({"args": [], "kwargs": {"workspace_id": id}});
        let read = |payload: &Value| {
            kwargs(payload)
                .and_then(|kw| uuid_value(kw.get("workspace_id")))
                .or_else(|| crate::args(payload).and_then(|args| uuid_value(args.first())))
        };
        assert_eq!(read(&positional), Some(Uuid::parse_str(id).expect("uuid")));
        assert_eq!(read(&keyword), Some(Uuid::parse_str(id).expect("uuid")));
    }

    #[test]
    fn the_defaults_are_the_models_json_defaults() {
        let value: Value = serde_json::from_str(DEFAULT_PROPS).expect("default props");
        assert_eq!(
            value["display_filters"]["layout"],
            Value::String("list".to_string())
        );
        let value: Value = serde_json::from_str(ISSUE_PROPS).expect("issue props");
        assert_eq!(value["all_issues"], Value::Bool(true));
    }
}
