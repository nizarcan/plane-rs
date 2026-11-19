// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-admin`: the Django management commands that are neither routes nor
//! queue tasks (`plans/CUTOVER.md` §1 line 72, §3).
//!
//! Today: the two version backfills.
//!
//! * `sync_issue_version` (`plane/bgtasks/issue_version_sync.py:181-222`) and
//!   its `schedule_issue_version` entry point (`:236`) are only ever called by
//!   `manage.py sync_issue_version` (`plane/db/management/commands/
//!   sync_issue_version.py:11-22`). No HTTP route, beat row or task publishes
//!   them, so they never get a `plane_jobs` row (`plans/CUTOVER.md` §1 line
//!   72).
//! * `sync_issue_description_version` (`plane/bgtasks/
//!   issue_description_version_sync.py:40-125`) and its `schedule_*` entry
//!   point (`:129-130`) are the same shape.
//!
//! The Python command only enqueues the scheduler and the scheduler enqueues
//! the first batch; every batch re-enqueues the next with `countdown`. A Rust
//! binary cannot leave that chain to Celery after the cutover, so the
//! subcommand *is* the chain: it walks the batches in this process and sleeps
//! `--countdown` seconds between them (the Celery `eta`). `--batch-size`
//! replaces the command's `input("Enter the batch size: ")`.
//!
//! Differences from the Python operator flow, all deliberate:
//!
//! * the string `batch_size` round trip through the command's `input()` does
//!   not exist — the value is an integer here and `--batch-size 5000` is the
//!   same batch the task ran;
//! * `--countdown 0` runs the whole backfill without sleeping (the
//!   differential check uses it);
//! * the command prints a confirmation and exits while the chain runs in the
//!   worker; the binary prints one line per batch.
//!
//! Each batch keeps the Python task's transaction and ordering:
//! `Issue.objects` is `SoftDeletionManager`, the batch is
//! `order_by("created_at")[offset:end_offset]` of the live issues, and the
//! re-enqueue condition is `end_offset < total_issues_count`. The batch's
//! `bulk_create` and the read of `total_issues_count` are in one transaction.

use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, NaiveDate, Utc};
use clap::{Parser, Subcommand};
use plane_core::config::{Role, Settings};
use plane_db::{DbPools, JsonText};
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "plane-admin",
    about = "Operator commands for the Rust Plane backend"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create IssueVersion rows for existing issues, in batches
    /// (`manage.py sync_issue_version`).
    SyncIssueVersion {
        /// Issues per batch (`sync_issue_version`'s `batch_size`, default 5000).
        #[arg(long, default_value_t = 5000)]
        batch_size: i64,
        /// Seconds between batches (the Celery chain's `countdown`, default 300).
        #[arg(long, default_value_t = 300)]
        countdown: u64,
    },
    /// Create IssueDescriptionVersion rows for existing issues, in batches
    /// (`manage.py sync_issue_description_version`).
    SyncIssueDescriptionVersion {
        /// Issues per batch (default 5000).
        #[arg(long, default_value_t = 5000)]
        batch_size: i64,
        /// Seconds between batches (default 300).
        #[arg(long, default_value_t = 300)]
        countdown: u64,
    },
    /// Reset a user's password (`manage.py reset_password <email>`).
    ResetPassword {
        /// The user's e-mail address.
        email: String,
    },
    /// Make a user active (`manage.py activate_user <email>`).
    ActivateUser {
        /// The user's e-mail address.
        email: String,
    },
    /// Send the test e-mail (`manage.py test_email <to_email>`).
    TestEmail {
        /// The receiver's e-mail address.
        to_email: String,
    },
    /// Create or extend the `Instance` row (`manage.py register_instance`).
    /// This is the API's boot step, exposed for operators and the comparison
    /// harness. `PLANE_RS_GITHUB_RELEASES_URL` points the version lookup at a
    /// local fake in tests (ENV.md §6).
    RegisterInstance {
        /// The entrypoint's machine signature; only checked for emptiness,
        /// exactly as Django does.
        machine_signature: String,
    },
    /// Seed the 36 `InstanceConfiguration` rows (`manage.py configure_instance`).
    ConfigureInstance,
    /// Head/create the object-storage bucket (`manage.py create_bucket`).
    CreateBucket,
    /// `FLUSHDB` on `REDIS_URL` (`manage.py clear_cache`).
    ClearCache,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        // Django's logging goes to stderr; stdout stays the command output the
        // differential comparison reads byte for byte.
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let cli = Cli::parse();
    // `clear_cache` is the one subcommand that needs `REDIS_URL`; the rest are
    // Django commands that run with only a database (and `SECRET_KEY`).
    let role = match cli.command {
        Command::ClearCache => Role::Api,
        _ => Role::Migrator,
    };
    let settings = Settings::from_env(role).map_err(|error| anyhow::anyhow!("{error}"))?;
    let db = DbPools::from_settings(&settings)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the tokio runtime")?;
    runtime.block_on(async move {
        let mut client = db
            .write_pool()
            .get()
            .await
            .context("connecting to the database")?;
        match cli.command {
            Command::SyncIssueVersion {
                batch_size,
                countdown,
            } => sync_issue_version(&mut client, batch_size, Duration::from_secs(countdown)).await,
            Command::SyncIssueDescriptionVersion {
                batch_size,
                countdown,
            } => {
                sync_issue_description_version(
                    &mut client,
                    batch_size,
                    Duration::from_secs(countdown),
                )
                .await
            }
            Command::ResetPassword { email } => {
                finish(plane_bootstrap::admin::reset_password(&client, &email).await?)
            }
            Command::ActivateUser { email } => {
                finish(plane_bootstrap::admin::activate_user(&client, &email).await?)
            }
            Command::TestEmail { to_email } => {
                finish(plane_bootstrap::admin::test_email(&client, &settings, &to_email).await?)
            }
            Command::RegisterInstance { machine_signature } => {
                plane_bootstrap::instance::register_instance(
                    &client,
                    &settings,
                    &machine_signature,
                )
                .await?;
                Ok(())
            }
            Command::ConfigureInstance => {
                plane_bootstrap::instance::configure_instance(&client, &settings).await?;
                Ok(())
            }
            Command::CreateBucket => {
                plane_bootstrap::bucket::create_bucket(&settings).await;
                Ok(())
            }
            Command::ClearCache => {
                plane_bootstrap::cache::clear_cache(&settings).await;
                Ok(())
            }
        }
    })
}

/// Django's `CommandError`: `manage.py` prints `CommandError: <message>` to
/// stderr and exits 1.
fn finish(
    result: std::result::Result<(), plane_bootstrap::admin::AdminError>,
) -> anyhow::Result<()> {
    match result {
        Ok(()) => Ok(()),
        Err(plane_bootstrap::admin::AdminError::CommandError(message)) => {
            eprintln!("CommandError: {message}");
            std::process::exit(1);
        }
    }
}

/// `sync_issue_version`'s batch loop (`issue_version_sync.py:181-222`).
async fn sync_issue_version(
    client: &mut Client,
    batch_size: i64,
    countdown: Duration,
) -> anyhow::Result<()> {
    anyhow::ensure!(batch_size > 0, "--batch-size must be positive");
    let mut offset: i64 = 0;
    loop {
        // `total_issues_count = Issue.objects.count()`; the default manager is
        // `SoftDeletionManager` (`plane/db/mixins.py:52-55`), so soft-deleted
        // issues are not backfilled.
        let total: i64 = client
            .query_one("SELECT count(*) FROM issues WHERE deleted_at IS NULL", &[])
            .await
            .context("counting issues")?
            .get(0);
        if total == 0 || offset >= total {
            return Ok(());
        }
        let end_offset = (offset + batch_size).min(total);
        let batch = fetch_issue_batch(client, offset, end_offset).await?;
        if batch.is_empty() {
            return Ok(());
        }
        let related = RelatedData::load(client, &batch).await?;

        let transaction = client
            .transaction()
            .await
            .context("opening the batch transaction")?;
        let mut created = 0usize;
        for issue in &batch {
            // `create_issue_version`'s `try/except`: an issue without a
            // resolvable owner is skipped, not an error
            // (`issue_version_sync.py:136-141`).
            let owned_by = owner_id(
                &transaction,
                issue.project_id,
                issue.updated_by_id,
                issue.created_by_id,
            )
            .await?;
            let Some(owned_by) = owned_by else {
                tracing::warn!(
                    target: "plane.admin",
                    issue = %issue.id,
                    "skipping issue without an owner"
                );
                continue;
            };
            let id = Uuid::new_v4();
            let now = Utc::now();
            transaction
                .execute(
                    "INSERT INTO issue_versions \
                     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                      project_id, workspace_id, parent, state, estimate_point, name, priority, \
                      start_date, target_date, assignees, sequence_id, labels, sort_order, \
                      completed_at, archived_at, is_draft, external_source, external_id, type, \
                      cycle, modules, properties, meta, last_saved_at, issue_id, activity_id, \
                      owned_by_id) \
                     VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, $12, $13, \
                             $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, \
                             $27, $28, $29, $30, $31, $32)",
                    &[
                        &id,
                        &now,
                        &now,
                        &issue.created_by_id,
                        &issue.updated_by_id,
                        &issue.project_id,
                        &issue.workspace_id,
                        &issue.parent_id,
                        &issue.state_id,
                        &issue.estimate_point_id,
                        &issue.name,
                        &issue.priority,
                        &issue.start_date,
                        &issue.target_date,
                        &related.assignees(issue.id),
                        &issue.sequence_id,
                        &related.labels(issue.id),
                        &issue.sort_order,
                        &issue.completed_at,
                        &issue.archived_at,
                        &issue.is_draft,
                        &issue.external_source,
                        &issue.external_id,
                        &issue.type_id,
                        &related.cycle(issue.id),
                        &related.modules(issue.id),
                        // `getattr(issue, "properties", {})`: `Issue` has no
                        // such fields, so the version always stores `{}`.
                        &JsonText::new("{}"),
                        &JsonText::new("{}"),
                        &now,
                        &issue.id,
                        &related.activity(issue.id),
                        &owned_by,
                    ],
                )
                .await
                .context("inserting an issue version")?;
            created += 1;
        }
        transaction.commit().await.context("committing the batch")?;
        tracing::info!(
            target: "plane.admin",
            offset,
            end_offset,
            created,
            "Processed Issues"
        );
        offset = end_offset;
        if offset >= total {
            return Ok(());
        }
        if !countdown.is_zero() {
            tokio::time::sleep(countdown).await;
        }
    }
}

/// One `IssueVersion` source row: the fields `create_issue_version` copies
/// (`issue_version_sync.py:126-173`).
struct IssueRow {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    created_by_id: Option<Uuid>,
    updated_by_id: Option<Uuid>,
    parent_id: Option<Uuid>,
    state_id: Option<Uuid>,
    estimate_point_id: Option<Uuid>,
    name: String,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    archived_at: Option<NaiveDate>,
    is_draft: bool,
    external_source: Option<String>,
    external_id: Option<String>,
    type_id: Option<Uuid>,
}

async fn fetch_issue_batch(
    client: &Client,
    offset: i64,
    end_offset: i64,
) -> anyhow::Result<Vec<IssueRow>> {
    let rows = client
        .query(
            "SELECT id, workspace_id, project_id, created_by_id, updated_by_id, parent_id, \
                    state_id, estimate_point_id, name, priority, start_date, target_date, \
                    sequence_id, sort_order, completed_at, archived_at, is_draft, \
                    external_source, external_id, type_id \
             FROM issues WHERE deleted_at IS NULL \
             ORDER BY created_at ASC OFFSET $1 LIMIT $2",
            &[&offset, &(end_offset - offset)],
        )
        .await
        .context("reading an issue batch")?;
    rows.iter()
        .map(|row| {
            Ok(IssueRow {
                id: row.try_get(0)?,
                workspace_id: row.try_get(1)?,
                project_id: row.try_get(2)?,
                created_by_id: row.try_get(3)?,
                updated_by_id: row.try_get(4)?,
                parent_id: row.try_get(5)?,
                state_id: row.try_get(6)?,
                estimate_point_id: row.try_get(7)?,
                name: row.try_get(8)?,
                priority: row.try_get(9)?,
                start_date: row.try_get(10)?,
                target_date: row.try_get(11)?,
                sequence_id: row.try_get(12)?,
                sort_order: row.try_get(13)?,
                completed_at: row.try_get(14)?,
                archived_at: row.try_get(15)?,
                is_draft: row.try_get(16)?,
                external_source: row.try_get(17)?,
                external_id: row.try_get(18)?,
                type_id: row.try_get(19)?,
            })
        })
        .collect()
}

/// `get_related_data` (`issue_version_sync.py:68-123`).
struct RelatedData {
    /// `{issue_id: cycle_id}` from `CycleIssue.objects.filter(...)`; the
    /// queryset order is `-created_at` and the dict comprehension keeps the
    /// last row, i.e. the *oldest* cycle link.
    cycles: std::collections::HashMap<Uuid, Uuid>,
    assignees: std::collections::HashMap<Uuid, Vec<Uuid>>,
    labels: std::collections::HashMap<Uuid, Vec<Uuid>>,
    modules: std::collections::HashMap<Uuid, Vec<Uuid>>,
    activities: std::collections::HashMap<Uuid, Uuid>,
}

impl RelatedData {
    async fn load(client: &Client, batch: &[IssueRow]) -> anyhow::Result<Self> {
        let ids: Vec<Uuid> = batch.iter().map(|issue| issue.id).collect();

        let cycle_rows = client
            .query(
                "SELECT issue_id, cycle_id FROM cycle_issues \
                 WHERE issue_id = ANY($1) AND deleted_at IS NULL \
                 ORDER BY issue_id, created_at DESC",
                &[&ids],
            )
            .await
            .context("reading cycle links")?;
        let mut cycles = std::collections::HashMap::new();
        for row in cycle_rows {
            // Later rows overwrite earlier ones; `-created_at` means the
            // oldest wins.
            cycles.insert(row.get(0), row.get(1));
        }

        let assignees = grouped(client, ids.as_slice(), "issue_assignees", "assignee_id").await?;
        let labels = grouped(client, ids.as_slice(), "issue_labels", "label_id").await?;
        let modules = grouped(client, ids.as_slice(), "module_issues", "module_id").await?;

        let activity_rows = client
            .query(
                "SELECT DISTINCT ON (issue_id) issue_id, id FROM issue_activities \
                 WHERE issue_id = ANY($1) AND deleted_at IS NULL \
                 ORDER BY issue_id, created_at DESC",
                &[&ids],
            )
            .await
            .context("reading the latest activities")?;
        let activities = activity_rows
            .into_iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect();
        Ok(Self {
            cycles,
            assignees,
            labels,
            modules,
            activities,
        })
    }

    fn cycle(&self, issue: Uuid) -> Option<Uuid> {
        self.cycles.get(&issue).copied()
    }

    fn activity(&self, issue: Uuid) -> Option<Uuid> {
        self.activities.get(&issue).copied()
    }

    fn assignees(&self, issue: Uuid) -> &[Uuid] {
        self.assignees.get(&issue).map_or(&[], Vec::as_slice)
    }

    fn labels(&self, issue: Uuid) -> &[Uuid] {
        self.labels.get(&issue).map_or(&[], Vec::as_slice)
    }

    fn modules(&self, issue: Uuid) -> &[Uuid] {
        self.modules.get(&issue).map_or(&[], Vec::as_slice)
    }
}

/// `values_list("issue_id", target).order_by("issue_id")`, grouped in row
/// order (`issue_version_sync.py:75-99`).
async fn grouped(
    client: &Client,
    ids: &[Uuid],
    table: &str,
    column: &str,
) -> anyhow::Result<std::collections::HashMap<Uuid, Vec<Uuid>>> {
    // `table`/`column` are constants from the callers above.
    let sql = format!(
        "SELECT issue_id, {column} FROM {table} \
         WHERE issue_id = ANY($1) AND deleted_at IS NULL ORDER BY issue_id"
    );
    let rows = client
        .query(&sql, &[&ids])
        .await
        .with_context(|| format!("reading {table}"))?;
    let mut grouped: std::collections::HashMap<Uuid, Vec<Uuid>> = std::collections::HashMap::new();
    for row in rows {
        grouped.entry(row.get(0)).or_default().push(row.get(1));
    }
    Ok(grouped)
}

/// `get_owner_id` (`issue_version_sync.py:45-62`): the issue's `updated_by`,
/// then `created_by`, then the newest project admin (role 20).
async fn owner_id<C: tokio_postgres::GenericClient + Sync>(
    client: &C,
    project_id: Uuid,
    updated_by_id: Option<Uuid>,
    created_by_id: Option<Uuid>,
) -> anyhow::Result<Option<Uuid>> {
    if let Some(owner) = updated_by_id {
        return Ok(Some(owner));
    }
    if let Some(owner) = created_by_id {
        return Ok(Some(owner));
    }
    let row = client
        .query_opt(
            "SELECT member_id FROM project_members \
             WHERE project_id = $1 AND role = 20 AND deleted_at IS NULL \
             ORDER BY created_at DESC LIMIT 1",
            &[&project_id],
        )
        .await
        .context("reading the project admin")?;
    Ok(row.and_then(|row| row.get::<_, Option<Uuid>>(0)))
}

/// `sync_issue_description_version`'s batch loop
/// (`issue_description_version_sync.py:40-125`). Simpler than the version
/// backfill: there is no related data, only the issue's description columns.
async fn sync_issue_description_version(
    client: &mut Client,
    batch_size: i64,
    countdown: Duration,
) -> anyhow::Result<()> {
    anyhow::ensure!(batch_size > 0, "--batch-size must be positive");
    let mut offset: i64 = 0;
    loop {
        let total: i64 = client
            .query_one("SELECT count(*) FROM issues WHERE deleted_at IS NULL", &[])
            .await
            .context("counting issues")?
            .get(0);
        if total == 0 || offset >= total {
            return Ok(());
        }
        let end_offset = (offset + batch_size).min(total);
        let rows = client
            .query(
                "SELECT id, workspace_id, project_id, created_by_id, updated_by_id, \
                        description_binary, description_html, description_stripped, \
                        description_json \
                 FROM issues WHERE deleted_at IS NULL \
                 ORDER BY created_at ASC OFFSET $1 LIMIT $2",
                &[&offset, &(end_offset - offset)],
            )
            .await
            .context("reading an issue batch")?;
        if rows.is_empty() {
            return Ok(());
        }

        let transaction = client
            .transaction()
            .await
            .context("opening the batch transaction")?;
        let mut created = 0usize;
        for row in &rows {
            let issue_id: Uuid = row.get(0);
            let workspace_id: Uuid = row.get(1);
            let project_id: Uuid = row.get(2);
            let created_by_id: Option<Uuid> = row.get(3);
            let updated_by_id: Option<Uuid> = row.get(4);
            let description_binary: Option<Vec<u8>> = row.get(5);
            let description_html: String = row.get(6);
            let description_stripped: Option<String> = row.get(7);
            let description_json: JsonText = row.get(8);

            let owner = owner_id(&transaction, project_id, updated_by_id, created_by_id).await?;
            let Some(owned_by) = owner else {
                tracing::warn!(
                    target: "plane.admin",
                    issue = %issue_id,
                    "skipping issue without an owner"
                );
                continue;
            };
            let id = Uuid::new_v4();
            let now = Utc::now();
            transaction
                .execute(
                    "INSERT INTO issue_description_versions \
                     (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                      project_id, workspace_id, issue_id, description_binary, description_html, \
                      description_stripped, description_json, last_saved_at, owned_by_id) \
                     VALUES ($1, $2, $3, $4, $5, NULL, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
                    &[
                        &id,
                        &now,
                        &now,
                        &created_by_id,
                        &updated_by_id,
                        &project_id,
                        &workspace_id,
                        &issue_id,
                        &description_binary,
                        &description_html,
                        &description_stripped,
                        &description_json,
                        &now,
                        &owned_by,
                    ],
                )
                .await
                .context("inserting an issue description version")?;
            created += 1;
        }
        transaction.commit().await.context("committing the batch")?;
        tracing::info!(
            target: "plane.admin",
            offset,
            end_offset,
            created,
            "Processed Issues"
        );
        offset = end_offset;
        if offset >= total {
            return Ok(());
        }
        if !countdown.is_zero() {
            tokio::time::sleep(countdown).await;
        }
    }
}
