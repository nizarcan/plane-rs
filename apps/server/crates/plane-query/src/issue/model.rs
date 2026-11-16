// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The tables, relations and columns the issue read path touches.
//!
//! Table and column names are the Django schema's (`apps/server` never alters
//! it). A relation is joined at most once per statement, under a stable alias,
//! exactly as Django's join reuse does; requiring the same relation twice —
//! once for a filter, once for an ordering annotation — reuses the join and
//! promotes it from `LEFT OUTER` to `INNER` when a filter needs a match.

use crate::sql::{Frag, Join, JoinKind, Value};

/// Every relation the issue list can reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Relation {
    /// `issues.state` — `LEFT OUTER` for the manager's triage exclusion,
    /// promoted to `INNER` when a state filter needs a match.
    States,
    /// `issues.project` — always `INNER`: `Issue.issue_objects` excludes
    /// archived projects with a non-null FK, which Django expresses as an
    /// inner join plus `projects.archived_at IS NULL`.
    Projects,
    /// `issues.workspace` — `INNER`, for `workspace__slug`.
    Workspaces,
    /// `issues.labels` through `issue_labels`.
    IssueLabels,
    /// `issue_labels.label` — only for `labels__name` ordering.
    Labels,
    /// `issues.assignees` through `issue_assignees`.
    IssueAssignees,
    /// `issue_assignees.assignee` — only for `assignees__first_name` ordering.
    Users,
    /// `issues.issue_module` through `module_issues`.
    ModuleIssues,
    /// `module_issues.module` — only for `issue_module__module__name` ordering.
    Modules,
    /// `issues.issue_cycle` through `cycle_issues`.
    CycleIssues,
    /// `issues.issue_subscribers` through `issue_subscribers`.
    IssueSubscribers,
    /// `issues.issue_intake` through `intake_issues`.
    IntakeIssues,
    /// `issues.issue_mention` through `issue_mentions`.
    IssueMentions,
}

impl Relation {
    pub fn table(self) -> &'static str {
        match self {
            Self::States => "states",
            Self::Projects => "projects",
            Self::Workspaces => "workspaces",
            Self::IssueLabels => "issue_labels",
            Self::Labels => "labels",
            Self::IssueAssignees => "issue_assignees",
            Self::Users => "users",
            Self::ModuleIssues => "module_issues",
            Self::Modules => "modules",
            Self::CycleIssues => "cycle_issues",
            Self::IssueSubscribers => "issue_subscribers",
            Self::IntakeIssues => "intake_issues",
            Self::IssueMentions => "issue_mentions",
        }
    }

    pub fn alias(self) -> &'static str {
        self.table()
    }

    /// The relation that must be joined before this one.
    pub fn depends_on(self) -> Option<Relation> {
        match self {
            Self::Labels => Some(Self::IssueLabels),
            Self::Users => Some(Self::IssueAssignees),
            Self::Modules => Some(Self::ModuleIssues),
            _ => None,
        }
    }

    /// The `ON` clause, with table-qualified column names.
    pub fn on(self) -> Frag {
        match self {
            Self::States => Frag::raw("issues.state_id = states.id"),
            Self::Projects => Frag::raw("issues.project_id = projects.id"),
            Self::Workspaces => Frag::raw("issues.workspace_id = workspaces.id"),
            Self::IssueLabels => Frag::raw("issues.id = issue_labels.issue_id"),
            Self::Labels => Frag::raw("issue_labels.label_id = labels.id"),
            Self::IssueAssignees => Frag::raw("issues.id = issue_assignees.issue_id"),
            Self::Users => Frag::raw("issue_assignees.assignee_id = users.id"),
            Self::ModuleIssues => Frag::raw("issues.id = module_issues.issue_id"),
            Self::Modules => Frag::raw("module_issues.module_id = modules.id"),
            Self::CycleIssues => Frag::raw("issues.id = cycle_issues.issue_id"),
            Self::IssueSubscribers => Frag::raw("issues.id = issue_subscribers.issue_id"),
            Self::IntakeIssues => Frag::raw("issues.id = intake_issues.issue_id"),
            Self::IssueMentions => Frag::raw("issues.id = issue_mentions.issue_id"),
        }
    }

    /// The join kind to use when nothing has required a match yet.
    pub fn default_kind(self) -> JoinKind {
        match self {
            Self::Projects | Self::Workspaces => JoinKind::Inner,
            _ => JoinKind::Left,
        }
    }
}

/// The set of joins a statement needs.
///
/// Insertion order is preserved, so the emitted SQL is stable and dependencies
/// (a through table before its target) are always joined in order.
#[derive(Debug, Clone, Default)]
pub struct JoinSet {
    joins: Vec<(Relation, JoinKind)>,
}

impl JoinSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Require `relation`; `kind` can only strengthen an existing join.
    pub fn require(&mut self, relation: Relation, kind: JoinKind) {
        if let Some(dependency) = relation.depends_on() {
            self.require(dependency, JoinKind::Left);
        }
        if let Some(slot) = self.joins.iter_mut().find(|(item, _)| *item == relation) {
            if kind == JoinKind::Inner {
                slot.1 = JoinKind::Inner;
            }
            return;
        }
        self.joins.push((relation, kind));
    }

    /// Require an `INNER` join (a filter that needs a match).
    pub fn require_inner(&mut self, relation: Relation) {
        self.require(relation, JoinKind::Inner);
    }

    /// Require a `LEFT OUTER` join.
    pub fn require_left(&mut self, relation: Relation) {
        self.require(relation, JoinKind::Left);
    }

    pub fn contains(&self, relation: Relation) -> bool {
        self.joins.iter().any(|(item, _)| *item == relation)
    }

    /// The `(relation, kind)` pairs, in join order.
    pub fn entries(&self) -> Vec<(Relation, JoinKind)> {
        self.joins.clone()
    }

    pub fn to_joins(&self) -> Vec<Join> {
        self.joins
            .iter()
            .map(|(relation, kind)| Join {
                kind: *kind,
                table: relation.table(),
                alias: relation.alias(),
                on: relation.on(),
            })
            .collect()
    }
}

/// The states of `Issue.issue_objects` (see `MODEL_HOOKS.md` §1.4/§1.8).
///
/// ```
/// # use plane_query::issue::model::manager_predicate;
/// let sql = manager_predicate().into_sql();
/// assert!(sql.text.contains("issues.deleted_at IS NULL"));
/// ```
pub fn manager_predicate() -> Frag {
    Frag::is_null("issues.deleted_at")
        .and(Frag::raw(
            "(states.group IS NULL OR states.group <> 'triage')",
        ))
        .and(Frag::is_null("issues.archived_at"))
        .and(Frag::is_null("projects.archived_at"))
        .and(Frag::raw("NOT issues.is_draft"))
}

/// One group-by field from `ISSUE_GROUP_BY_ALLOWLIST`
/// (`plane/utils/order_queryset.py:99`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupField {
    StateId,
    StateGroup,
    Priority,
    LabelsId,
    AssigneesId,
    ModuleId,
    CycleId,
    ProjectId,
    CreatedBy,
    TargetDate,
    StartDate,
}

/// The allowlist, in the order the Python declares it (the paginator only
/// tests membership, but a stable order keeps error messages predictable).
pub const GROUP_BY_ALLOWLIST: [GroupField; 11] = [
    GroupField::StateId,
    GroupField::StateGroup,
    GroupField::Priority,
    GroupField::LabelsId,
    GroupField::AssigneesId,
    GroupField::ModuleId,
    GroupField::CycleId,
    GroupField::ProjectId,
    GroupField::CreatedBy,
    GroupField::TargetDate,
    GroupField::StartDate,
];

impl GroupField {
    /// Parse a `group_by` query parameter; `None` is not on the allowlist.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|field| field.as_str() == name)
    }

    const ALL: [Self; 11] = GROUP_BY_ALLOWLIST;

    /// The query-parameter spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateId => "state_id",
            Self::StateGroup => "state__group",
            Self::Priority => "priority",
            Self::LabelsId => "labels__id",
            Self::AssigneesId => "assignees__id",
            Self::ModuleId => "issue_module__module_id",
            Self::CycleId => "cycle_id",
            Self::ProjectId => "project_id",
            Self::CreatedBy => "created_by",
            Self::TargetDate => "target_date",
            Self::StartDate => "start_date",
        }
    }

    /// `GroupedOffsetPaginator.FIELD_MAPPER`'s value, for the m2m group fields:
    /// the name of the values annotation the raw column replaces.
    pub fn mapper(self) -> Option<&'static str> {
        match self {
            Self::LabelsId => Some("label_ids"),
            Self::AssigneesId => Some("assignee_ids"),
            Self::ModuleId => Some("module_ids"),
            _ => None,
        }
    }

    /// The relation the group expression needs, if any.
    pub fn relation(self) -> Option<Relation> {
        match self {
            Self::StateGroup => Some(Relation::States),
            Self::LabelsId => Some(Relation::IssueLabels),
            Self::AssigneesId => Some(Relation::IssueAssignees),
            Self::ModuleId => Some(Relation::ModuleIssues),
            _ => None,
        }
    }

    /// The SQL expression the group key is computed from.
    ///
    /// `cycle_id` is an annotation, not a column: `IssueViewSet.apply_annotations`
    /// adds `Subquery(CycleIssue…values("cycle_id")[:1])`, ordered by
    /// `CycleIssue.Meta.ordering` (`-created_at`), so only the newest
    /// non-deleted cycle link is reported.
    pub fn expression(self) -> Frag {
        match self {
            Self::StateId => Frag::raw("issues.state_id"),
            Self::StateGroup => Frag::raw("states.group"),
            Self::Priority => Frag::raw("issues.priority"),
            Self::LabelsId => Frag::raw("issue_labels.label_id"),
            Self::AssigneesId => Frag::raw("issue_assignees.assignee_id"),
            Self::ModuleId => Frag::raw("module_issues.module_id"),
            Self::CycleId => cycle_id_expression(),
            Self::ProjectId => Frag::raw("issues.project_id"),
            Self::CreatedBy => Frag::raw("issues.created_by_id"),
            Self::TargetDate => Frag::raw("issues.target_date"),
            Self::StartDate => Frag::raw("issues.start_date"),
        }
    }

    /// Whether the group value can be `None` (used by `issue_group_values` and
    /// by the multi-grouper's `"None"` special case).
    pub fn nullable(self) -> bool {
        true
    }

    /// The `issue_on_results` field that carries this group's key.
    ///
    /// For an m2m field the annotation is replaced by the raw column; every
    /// other group field is already part of the base projection under its own
    /// name (`state_id`, `priority`, `cycle_id`, …).
    pub fn value_field(self) -> ValueField {
        match self {
            Self::StateId => ValueField::StateId,
            Self::StateGroup => ValueField::StateGroup,
            Self::Priority => ValueField::Priority,
            Self::LabelsId => ValueField::Group(Self::LabelsId),
            Self::AssigneesId => ValueField::Group(Self::AssigneesId),
            Self::ModuleId => ValueField::Group(Self::ModuleId),
            Self::CycleId => ValueField::CycleId,
            Self::ProjectId => ValueField::ProjectId,
            Self::CreatedBy => ValueField::CreatedBy,
            Self::TargetDate => ValueField::TargetDate,
            Self::StartDate => ValueField::StartDate,
        }
    }
}

/// `Subquery(CycleIssue.objects.filter(issue=OuterRef("id"), deleted_at…).values("cycle_id")[:1])`.
pub fn cycle_id_expression() -> Frag {
    Frag::raw(
        "(SELECT cycle_issues.cycle_id FROM cycle_issues \
         WHERE cycle_issues.deleted_at IS NULL AND cycle_issues.issue_id = issues.id \
         ORDER BY cycle_issues.created_at DESC LIMIT 1)",
    )
}

/// The columns `issue_on_results` projects, in order
/// (`plane/utils/grouper.py:104-141`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueField {
    Id,
    Name,
    StateId,
    SortOrder,
    CompletedAt,
    EstimatePoint,
    Priority,
    StartDate,
    TargetDate,
    SequenceId,
    ProjectId,
    ParentId,
    CycleId,
    SubIssuesCount,
    CreatedAt,
    UpdatedAt,
    CreatedBy,
    UpdatedBy,
    AttachmentCount,
    LinkCount,
    IsDraft,
    ArchivedAt,
    StateGroup,
    AssigneeIds,
    LabelIds,
    ModuleIds,
    /// The raw group column that replaces its annotation when the query is
    /// grouped by an m2m field (`labels__id`, `assignees__id`,
    /// `issue_module__module_id`).
    Group(GroupField),
}

impl ValueField {
    /// The key in the result dict (and therefore in the response JSON).
    pub fn key(self) -> &'static str {
        match self {
            Self::Id => "id",
            Self::Name => "name",
            Self::StateId => "state_id",
            Self::SortOrder => "sort_order",
            Self::CompletedAt => "completed_at",
            Self::EstimatePoint => "estimate_point",
            Self::Priority => "priority",
            Self::StartDate => "start_date",
            Self::TargetDate => "target_date",
            Self::SequenceId => "sequence_id",
            Self::ProjectId => "project_id",
            Self::ParentId => "parent_id",
            Self::CycleId => "cycle_id",
            Self::SubIssuesCount => "sub_issues_count",
            Self::CreatedAt => "created_at",
            Self::UpdatedAt => "updated_at",
            Self::CreatedBy => "created_by",
            Self::UpdatedBy => "updated_by",
            Self::AttachmentCount => "attachment_count",
            Self::LinkCount => "link_count",
            Self::IsDraft => "is_draft",
            Self::ArchivedAt => "archived_at",
            Self::StateGroup => "state__group",
            Self::AssigneeIds => "assignee_ids",
            Self::LabelIds => "label_ids",
            Self::ModuleIds => "module_ids",
            Self::Group(field) => field.as_str(),
        }
    }

    /// The base projection, before the group substitution.
    pub fn base() -> Vec<Self> {
        vec![
            Self::Id,
            Self::Name,
            Self::StateId,
            Self::SortOrder,
            Self::CompletedAt,
            Self::EstimatePoint,
            Self::Priority,
            Self::StartDate,
            Self::TargetDate,
            Self::SequenceId,
            Self::ProjectId,
            Self::ParentId,
            Self::CycleId,
            Self::SubIssuesCount,
            Self::CreatedAt,
            Self::UpdatedAt,
            Self::CreatedBy,
            Self::UpdatedBy,
            Self::AttachmentCount,
            Self::LinkCount,
            Self::IsDraft,
            Self::ArchivedAt,
            Self::StateGroup,
            Self::AssigneeIds,
            Self::LabelIds,
            Self::ModuleIds,
        ]
    }

    /// `issue_on_results`'s field list for one (group, sub-group) pair.
    ///
    /// The three id annotations come last; grouping by one of them removes its
    /// annotation and appends the raw column instead.
    pub fn project(group_by: Option<GroupField>, sub_group_by: Option<GroupField>) -> Vec<Self> {
        let mut fields = Self::base();
        let mut substitutions = Vec::new();
        for group in [group_by, sub_group_by].into_iter().flatten() {
            if let Some(mapper) = group.mapper()
                && let Some(field) = match mapper {
                    "label_ids" => Some(Self::LabelIds),
                    "assignee_ids" => Some(Self::AssigneeIds),
                    "module_ids" => Some(Self::ModuleIds),
                    _ => None,
                }
            {
                fields.retain(|candidate| *candidate != field);
                substitutions.push(Self::Group(group));
            }
        }
        fields.extend(substitutions);
        fields
    }
}

/// `issues.state__group` needs the states join; expose the column for the
/// projection and for the `GROUP BY` Django adds when an aggregate annotation
/// puts the query in a grouped query.
pub fn state_group_column() -> &'static str {
    "states.group"
}

/// The `triage` state group (`StateGroup.TRIAGE`, `plane/db/models/state.py`).
pub fn triage_group() -> Value {
    Value::Text("triage".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_dependencies_come_first() {
        let mut joins = JoinSet::new();
        joins.require_inner(Relation::Labels);
        let names: Vec<&str> = joins.to_joins().iter().map(|join| join.alias).collect();
        assert_eq!(names, vec!["issue_labels", "labels"]);
        assert_eq!(joins.to_joins()[0].kind, JoinKind::Left);
        assert_eq!(joins.to_joins()[1].kind, JoinKind::Inner);
    }

    #[test]
    fn joins_promote_but_never_weaken() {
        let mut joins = JoinSet::new();
        joins.require_left(Relation::IssueLabels);
        joins.require_inner(Relation::IssueLabels);
        joins.require_left(Relation::IssueLabels);
        assert_eq!(joins.to_joins().len(), 1);
        assert_eq!(joins.to_joins()[0].kind, JoinKind::Inner);
    }

    #[test]
    fn value_fields_follow_issue_on_results() {
        let plain = ValueField::project(None, None);
        assert_eq!(plain.len(), 26);
        assert_eq!(plain[0].key(), "id");
        assert_eq!(plain[22].key(), "state__group");
        assert_eq!(
            plain[23..]
                .iter()
                .map(|field| field.key())
                .collect::<Vec<_>>(),
            vec!["assignee_ids", "label_ids", "module_ids"]
        );

        let labels = ValueField::project(Some(GroupField::LabelsId), None);
        assert_eq!(
            labels[23..]
                .iter()
                .map(|field| field.key())
                .collect::<Vec<_>>(),
            vec!["assignee_ids", "module_ids", "labels__id"]
        );

        let both = ValueField::project(Some(GroupField::LabelsId), Some(GroupField::ModuleId));
        assert_eq!(
            both[23..]
                .iter()
                .map(|field| field.key())
                .collect::<Vec<_>>(),
            vec!["assignee_ids", "labels__id", "issue_module__module_id"]
        );
    }

    #[test]
    fn group_fields_round_trip() {
        for field in GROUP_BY_ALLOWLIST {
            assert_eq!(GroupField::parse(field.as_str()), Some(field));
        }
        assert_eq!(GroupField::parse("issue_labels__id"), None);
    }
}
