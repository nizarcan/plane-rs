// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! SQL for the draft-issue writes (`plane/app/views/workspace/draft.py`).

use plane_db::Query;

/// The schema-check catalog. The statements here are bound by the handler and
/// covered by `write/drafts.yaml`; the catalog stays empty because they are
/// raw SQL rather than `plane_query!` declarations.
pub static ALL_QUERIES: &[&Query] = &[];

/// The active project role of the caller, scoped to the URL workspace.
pub const PROJECT_ROLE: &str = "SELECT role FROM project_members WHERE member_id = $1 AND workspace_id = $2 \
     AND project_id = $3 AND is_active = true AND deleted_at IS NULL LIMIT 1";

/// A workspace ADMIN, for the second half of `can_write_project`.
pub const WORKSPACE_ADMIN: &str = "SELECT 1 FROM workspace_members WHERE member_id = $1 AND workspace_id = $2 \
     AND role = 20 AND is_active = true AND deleted_at IS NULL LIMIT 1";

/// `DraftIssue.objects.get` on the PATCH path (`created_by = request.user`).
pub const DRAFT_BY_ID_FOR_USER: &str = "SELECT created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, workspace_id, project_id, parent_id, state_id, \
     estimate_point_id, name, description_json, description_html, description_stripped, \
     description_binary, priority, start_date, target_date, sort_order, completed_at, \
     external_source, external_id, type_id FROM draft_issues WHERE id = $1 \
     AND workspace_id = $2 AND created_by_id = $3 AND deleted_at IS NULL";

/// `DraftIssue.objects.get(workspace__slug=slug, pk=pk)` on DELETE.
pub const DRAFT_BY_ID: &str = "SELECT created_at, updated_at, created_by_id, updated_by_id, \
     deleted_at, id, workspace_id, project_id, parent_id, state_id, estimate_point_id, name, \
     description_json, description_html, description_stripped, description_binary, priority, \
     start_date, target_date, sort_order, completed_at, external_source, external_id, type_id \
     FROM draft_issues WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL";

pub const DRAFT_INSERT: &str = "INSERT INTO draft_issues (created_at, updated_at, created_by_id, \
     updated_by_id, deleted_at, id, workspace_id, project_id, parent_id, state_id, \
     estimate_point_id, name, description_json, description_html, description_stripped, \
     description_binary, priority, start_date, target_date, sort_order, completed_at, \
     external_source, external_id, type_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
     $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24)";

pub const DRAFT_UPDATE: &str = "UPDATE draft_issues SET created_at = $1, updated_at = $2, \
     created_by_id = $3, updated_by_id = $4, deleted_at = $5, workspace_id = $6, \
     project_id = $7, parent_id = $8, state_id = $9, estimate_point_id = $10, name = $11, \
     description_json = $12, description_html = $13, description_stripped = $14, \
     description_binary = $15, priority = $16, start_date = $17, target_date = $18, \
     sort_order = $19, completed_at = $20, external_source = $21, external_id = $22, \
     type_id = $23 WHERE id = $24";

pub const STATE_IN_PROJECT: &str = "SELECT 1 FROM states WHERE id = $1 AND project_id = $2 AND is_triage = false \
     AND deleted_at IS NULL LIMIT 1";

pub const ISSUE_IN_PROJECT: &str =
    "SELECT 1 FROM issues WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL LIMIT 1";

pub const ESTIMATE_POINT_IN_PROJECT: &str = "SELECT 1 FROM estimate_points WHERE id = $1 AND project_id = $2 AND deleted_at IS NULL LIMIT 1";

pub const CYCLE_IN_PROJECT: &str = "SELECT 1 FROM cycles WHERE id = $1 AND project_id = $2 AND workspace_id = $3 \
     AND deleted_at IS NULL LIMIT 1";

pub const MODULES_IN_PROJECT: &str = "SELECT count(*) FROM modules WHERE id = ANY($1) AND project_id = $2 AND workspace_id = $3 \
     AND deleted_at IS NULL";

pub const ASSIGNEE_IDS: &str = "SELECT member_id FROM project_members WHERE project_id = $1 AND role >= 15 \
     AND is_active = true AND deleted_at IS NULL AND member_id = ANY($2)";

pub const LABEL_IDS: &str = "SELECT id FROM labels WHERE project_id = $1 \
     AND deleted_at IS NULL AND id = ANY($2)";

pub const SOFT_DELETE_ASSIGNEES: &str = "UPDATE draft_issue_assignees SET deleted_at = $1 WHERE draft_issue_id = $2 \
     AND deleted_at IS NULL";
pub const SOFT_DELETE_LABELS: &str = "UPDATE draft_issue_labels SET deleted_at = $1 WHERE draft_issue_id = $2 \
     AND deleted_at IS NULL";
pub const SOFT_DELETE_MODULES: &str = "UPDATE draft_issue_modules SET deleted_at = $1 WHERE draft_issue_id = $2 \
     AND deleted_at IS NULL";
pub const SOFT_DELETE_CYCLES: &str = "UPDATE draft_issue_cycles SET deleted_at = $1 WHERE draft_issue_id = $2 \
     AND deleted_at IS NULL";

pub const INSERT_ASSIGNEE: &str = "INSERT INTO draft_issue_assignees (created_at, updated_at, \
     deleted_at, id, assignee_id, created_by_id, draft_issue_id, project_id, updated_by_id, \
     workspace_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9)";
pub const INSERT_LABEL: &str = "INSERT INTO draft_issue_labels (created_at, updated_at, \
     deleted_at, id, created_by_id, draft_issue_id, label_id, project_id, updated_by_id, \
     workspace_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9)";
pub const INSERT_MODULE: &str = "INSERT INTO draft_issue_modules (created_at, updated_at, \
     deleted_at, id, created_by_id, draft_issue_id, module_id, project_id, updated_by_id, \
     workspace_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9)";
pub const INSERT_CYCLE: &str = "INSERT INTO draft_issue_cycles (created_at, updated_at, \
     deleted_at, id, created_by_id, cycle_id, draft_issue_id, project_id, updated_by_id, \
     workspace_id) VALUES ($1, $2, NULL, $3, $4, $5, $6, $7, $8, $9)";

/// The `create` response: the `.values(...)` projection with the same
/// annotations as `get_queryset()`.
pub const CREATE_RESPONSE: &str = "SELECT d.id, d.name, d.state_id, d.sort_order, \
     d.completed_at, d.estimate_point_id, d.priority, d.start_date, d.target_date, \
     d.project_id, d.parent_id, \
     (SELECT c.cycle_id FROM draft_issue_cycles c WHERE c.draft_issue_id = d.id \
       AND c.deleted_at IS NULL LIMIT 1), \
     COALESCE((SELECT array_agg(DISTINCT m.module_id) FROM draft_issue_modules m \
       JOIN modules mo ON mo.id = m.module_id WHERE m.draft_issue_id = d.id \
       AND m.deleted_at IS NULL AND mo.archived_at IS NULL), '{}'), \
     COALESCE((SELECT array_agg(DISTINCT l.label_id) FROM draft_issue_labels l \
       WHERE l.draft_issue_id = d.id AND l.deleted_at IS NULL), '{}'), \
     COALESCE((SELECT array_agg(DISTINCT a.assignee_id) FROM draft_issue_assignees a \
       JOIN project_members pm ON pm.member_id = a.assignee_id AND pm.project_id = a.project_id \
       AND pm.is_active = true WHERE a.draft_issue_id = d.id AND a.deleted_at IS NULL), '{}'), \
     d.created_at, d.updated_at, d.created_by_id, d.updated_by_id, d.type_id, d.description_html \
     FROM draft_issues d WHERE d.id = $1 AND d.deleted_at IS NULL";
