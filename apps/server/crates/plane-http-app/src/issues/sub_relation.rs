// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `GET …/issues/<issue_id>/issue-relation/` — `IssueRelationViewSet.list`
//! (route 93).
//!
//! Python: `plane/app/views/issue/relation.py:56-231`.
//!
//! The view answers one object with eight arrays (`blocking`, `blocked_by`,
//! `duplicate`, `relates_to`, `start_after`, `start_before`, `finish_after`,
//! `finish_before`). Each array is a `.values(*fields)` projection of
//! `Issue.issue_objects` — the manager excludes archived issues, draft issues
//! and issues in a `triage` state (`plane/db/models/issue.py:92-101`) — with
//! `relation_type` annotated to the *group's* name. The two symmetric groups
//! (`duplicate`, `relates_to`) are the union of both directions, which Django
//! builds with `qs1 | qs2` (an `OR` over two `IN` subqueries).
//!
//! Django runs one aggregate query per group (eight) on top of the relation
//! scan. This port reads the relation rows once and the referenced issues once,
//! then partitions in memory. That is seven round trips instead of thirteen.
//! The projection has no `ORDER BY` in Django either (`GROUP BY 1` only), so a
//! group with more than one issue has an unspecified order on both sides; the
//! fixture and the scenario keep one issue per group.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::{DrfDateTime, DrfUuidList};

use super::sub_common as shared;
use super::sub_queries as queries;
use crate::common;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.relation.IssueRelationViewSet")?
        .action("list", list)?;
    Ok(())
}

/// One `.values(*fields)` row, in the declaration order of
/// `IssueRelationViewSet.list`'s `fields` (`relation.py:178-208`).
#[derive(serde::Serialize)]
struct RelatedIssue<'a> {
    id: Uuid,
    name: &'a str,
    state_id: Option<Uuid>,
    sort_order: f64,
    priority: &'a str,
    sequence_id: i32,
    project_id: Uuid,
    label_ids: DrfUuidList<'a>,
    assignee_ids: DrfUuidList<'a>,
    // `.values()` rows go straight to the renderer's JSON encoder, which
    // writes the stored UTC datetime with `isoformat()`; only serializer
    // `DateTimeField`s apply the request timezone.
    created_at: DrfDateTime<Utc>,
    updated_at: DrfDateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    relation_type: &'static str,
}

struct IssueRow {
    id: Uuid,
    name: String,
    state_id: Option<Uuid>,
    sort_order: f64,
    priority: String,
    sequence_id: i32,
    project_id: Uuid,
    label_ids: Vec<Uuid>,
    assignee_ids: Vec<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
}

impl IssueRow {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0)?,
            name: row.try_get(1)?,
            state_id: row.try_get(2)?,
            sort_order: row.try_get(3)?,
            priority: row.try_get(4)?,
            sequence_id: row.try_get(5)?,
            project_id: row.try_get(6)?,
            label_ids: row.try_get(7)?,
            assignee_ids: row.try_get(8)?,
            created_at: row.try_get(9)?,
            updated_at: row.try_get(10)?,
            created_by: row.try_get(11)?,
            updated_by: row.try_get(12)?,
        })
    }

    fn serializer<'a>(&'a self, relation_type: &'static str) -> RelatedIssue<'a> {
        RelatedIssue {
            id: self.id,
            name: &self.name,
            state_id: self.state_id,
            sort_order: self.sort_order,
            priority: &self.priority,
            sequence_id: self.sequence_id,
            project_id: self.project_id,
            label_ids: DrfUuidList(&self.label_ids),
            assignee_ids: DrfUuidList(&self.assignee_ids),
            created_at: DrfDateTime(self.created_at),
            updated_at: DrfDateTime(self.updated_at),
            created_by: self.created_by,
            updated_by: self.updated_by,
            relation_type,
        }
    }
}

#[derive(Default)]
struct RelationSets {
    blocking: HashSet<Uuid>,
    blocked_by: HashSet<Uuid>,
    duplicate: HashSet<Uuid>,
    relates_to: HashSet<Uuid>,
    start_after: HashSet<Uuid>,
    start_before: HashSet<Uuid>,
    finish_after: HashSet<Uuid>,
    finish_before: HashSet<Uuid>,
}

impl RelationSets {
    /// The id sets `IssueRelationViewSet.list` derives from
    /// `issue_relations.filter(Q(issue_id=…) | Q(related_issue=…))`
    /// (`relation.py:100-177`).
    fn push(&mut self, issue_id: Uuid, related_issue_id: Uuid, relation_type: &str, target: Uuid) {
        let forward = issue_id == target;
        let reverse = related_issue_id == target;
        match relation_type {
            "blocked_by" => {
                if reverse {
                    self.blocking.insert(issue_id);
                }
                if forward {
                    self.blocked_by.insert(related_issue_id);
                }
            }
            "duplicate" => {
                if forward {
                    self.duplicate.insert(related_issue_id);
                }
                if reverse {
                    self.duplicate.insert(issue_id);
                }
            }
            "relates_to" => {
                if forward {
                    self.relates_to.insert(related_issue_id);
                }
                if reverse {
                    self.relates_to.insert(issue_id);
                }
            }
            "start_before" => {
                if reverse {
                    self.start_after.insert(issue_id);
                }
                if forward {
                    self.start_before.insert(related_issue_id);
                }
            }
            "finish_before" => {
                if reverse {
                    self.finish_after.insert(issue_id);
                }
                if forward {
                    self.finish_before.insert(related_issue_id);
                }
            }
            // `implemented_by` / `implements` have no group in the view.
            _ => {}
        }
    }

    fn all(&self) -> HashSet<Uuid> {
        let mut ids = HashSet::new();
        for set in [
            &self.blocking,
            &self.blocked_by,
            &self.duplicate,
            &self.relates_to,
            &self.start_after,
            &self.start_before,
            &self.finish_after,
            &self.finish_before,
        ] {
            ids.extend(set.iter().copied());
        }
        ids
    }
}

#[derive(serde::Serialize)]
struct RelationResponse<'a> {
    blocking: Vec<RelatedIssue<'a>>,
    blocked_by: Vec<RelatedIssue<'a>>,
    duplicate: Vec<RelatedIssue<'a>>,
    relates_to: Vec<RelatedIssue<'a>>,
    start_after: Vec<RelatedIssue<'a>>,
    start_before: Vec<RelatedIssue<'a>>,
    finish_after: Vec<RelatedIssue<'a>>,
    finish_before: Vec<RelatedIssue<'a>>,
}

/// `IssueRelationViewSet.list` (`relation.py:61-231`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id = shared::kwarg_uuid(ctx, "project_id")?;
        let issue_id = shared::kwarg_uuid(ctx, "issue_id")?;
        let user_id = shared::user_id(ctx)?;

        if !shared::issue_in_project(client, issue_id, project_id, slug).await? {
            return Ok(shared::issue_not_found_response());
        }
        if shared::issue_hidden_from_guest(client, slug, project_id, issue_id, user_id).await? {
            return Ok(shared::hidden_response());
        }

        let statement = queries::RELATIONS.prepare(client).await?;
        let relation_rows = client.query(&statement, &[&issue_id, &slug]).await?;
        let mut sets = RelationSets::default();
        for row in &relation_rows {
            let from: Uuid = row.try_get(0)?;
            let to: Uuid = row.try_get(1)?;
            let relation_type: String = row.try_get(2)?;
            sets.push(from, to, &relation_type, issue_id);
        }

        let ids = sets.all();
        let rows: Vec<IssueRow> = if ids.is_empty() {
            Vec::new()
        } else {
            let ids: Vec<Uuid> = ids.into_iter().collect();
            let statement = queries::RELATED_ISSUES.prepare(client).await?;
            let rows = client.query(&statement, &[&slug, &ids]).await?;
            rows.iter()
                .map(IssueRow::decode)
                .collect::<Result<_, _>>()?
        };

        let group = |set: &HashSet<Uuid>, relation_type: &'static str| -> Vec<RelatedIssue<'_>> {
            rows.iter()
                .filter(|row| set.contains(&row.id))
                .map(|row| row.serializer(relation_type))
                .collect()
        };
        let response = RelationResponse {
            blocking: group(&sets.blocking, "blocking"),
            blocked_by: group(&sets.blocked_by, "blocked_by"),
            duplicate: group(&sets.duplicate, "duplicate"),
            relates_to: group(&sets.relates_to, "relates_to"),
            start_after: group(&sets.start_after, "start_after"),
            start_before: group(&sets.start_before, "start_before"),
            finish_after: group(&sets.finish_after, "finish_after"),
            finish_before: group(&sets.finish_before, "finish_before"),
        };
        let body = plane_serde::to_vec(plane_serde::Dialect::Drf, &response)
            .map_err(|error| ViewError::Unknown(error.to_string()))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
