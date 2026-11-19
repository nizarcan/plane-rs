// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django's model graph, recorded from `plane/bgtasks/deletion_task.py`'s point
//! of view.
//!
//! The two deletion tasks do not carry lists of models: `soft_delete_related_objects`
//! walks `instance._meta.get_fields()` and `hard_delete` lets Django's
//! `Collector` walk every reverse relation. The port needs the same metadata,
//! so `tools/jobs/model_graph.py` records it with the pinned Django (5.2.17)
//! into `data/model_graph.json`, which this module parses once.
//!
//! Three things the Python cascade depends on and the table captures:
//!
//! * **Reverse relations only, and their `on_delete`.** `soft_delete_related_objects`
//!   skips `DO_NOTHING`, nulls `SET_NULL` and recurses into everything else
//!   (`deletion_task.py:32-100`).
//! * **The manager a branch uses.** The recursive branch resolves
//!   `getattr(instance, accessor)(manager="objects")` (`:83`), so the child's
//!   **named** `objects` manager filters; the `SET_NULL` branch uses the
//!   child's **default** manager (`:60`), and Django's hard-delete collector
//!   uses `_base_manager`, which is unfiltered for every Plane model. A wrong
//!   manager is a wrong row set (for example `IssueManager` hides triage,
//!   archived and draft issues, and `StateManager` hides triage states).
//! * **`apps.get_models()` order** for `hard_delete`'s final sweep
//!   (`deletion_task.py:184-191`), which is the app registry's order, not
//!   alphabetical.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// The JSON file `tools/jobs/model_graph.py` writes.
const GRAPH_JSON: &str = include_str!("../data/model_graph.json");

/// The manager class a queryset starts from, as far as the cascade cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    /// No predicate (Django's auto-created `_base_manager`, `all_objects`).
    Plain,
    /// `SoftDeletionManager`: `deleted_at IS NULL`.
    SoftDelete,
    /// `StateManager`: `deleted_at IS NULL AND "group" <> 'triage'`.
    State,
    /// `TriageStateManager`: `deleted_at IS NULL AND "group" = 'triage'`.
    TriageState,
    /// `IssueManager`: live, non-triage, non-archived, non-draft issues in
    /// non-archived projects (`plane/db/models/issue.py:92-101`).
    Issue,
}

impl Manager {
    /// Map the class name the generator recorded.
    #[must_use]
    pub fn from_class(class: &str) -> Self {
        match class {
            "SoftDeletionManager" => Self::SoftDelete,
            "StateManager" => Self::State,
            "TriageStateManager" => Self::TriageState,
            "IssueManager" => Self::Issue,
            _ => Self::Plain,
        }
    }

    /// The SQL predicate this manager applies, or the empty string for
    /// [`Manager::Plain`].
    ///
    /// The `Issue` predicate reproduces `IssueManager.get_queryset()`: the
    /// forward relations render as a `LEFT OUTER JOIN states` (so a `NULL`
    /// state is kept) and an `INNER JOIN projects` (so a missing project would
    /// drop the row, which the FK makes impossible).
    #[must_use]
    pub fn predicate(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::SoftDelete => "deleted_at IS NULL",
            Self::State => "deleted_at IS NULL AND \"group\" <> 'triage'",
            Self::TriageState => "deleted_at IS NULL AND \"group\" = 'triage'",
            Self::Issue => {
                "deleted_at IS NULL \
                 AND (state_id IS NULL OR state_id NOT IN \
                      (SELECT id FROM states WHERE \"group\" = 'triage')) \
                 AND archived_at IS NULL \
                 AND project_id IN (SELECT id FROM projects WHERE archived_at IS NULL) \
                 AND NOT is_draft"
            }
        }
    }
}

/// One reverse relation: a child model's FK pointing at this model.
#[derive(Clone, Debug, Deserialize)]
pub struct Child {
    /// Django model label (`db.issue`).
    pub model: String,
    /// Field name on the child (`issue`).
    pub field: String,
    /// Column on the child (`issue_id`).
    pub column: String,
    /// `get_accessor_name()`.
    pub accessor: String,
    /// `field.remote_field.on_delete.__name__`.
    pub on_delete: String,
    /// `OneToOneRel` rather than `ManyToOneRel`.
    pub one_to_one: bool,
    /// `field.null`.
    pub null: bool,
}

/// One foreign key on this model (the forward direction).
#[derive(Clone, Debug, Deserialize)]
pub struct Parent {
    /// Column on this model (`issue_id`).
    pub column: String,
    /// Field name (`issue`).
    pub field: String,
    /// Parent model label (`db.issue`).
    pub parent: String,
    pub on_delete: String,
    pub null: bool,
}

/// One named manager, for the `objects` lookup.
#[derive(Clone, Debug, Deserialize)]
pub struct ManagerEntry {
    pub name: String,
    pub class: String,
}

/// One model.
#[derive(Clone, Debug, Deserialize)]
pub struct Model {
    pub label: String,
    pub table: String,
    pub has_deleted_at: bool,
    pub default_manager: String,
    pub managers: Vec<ManagerEntry>,
    pub children: Vec<Child>,
    #[serde(default)]
    pub parents: Vec<Parent>,
}

impl Model {
    /// The manager class of the named manager (`getattr(model, name)`).
    #[must_use]
    pub fn manager(&self, name: &str) -> Manager {
        self.managers
            .iter()
            .find(|entry| entry.name == name)
            .map_or(Manager::Plain, |entry| Manager::from_class(&entry.class))
    }

    /// The manager `getattr(instance, accessor)(manager="objects")` resolves.
    #[must_use]
    pub fn objects(&self) -> Manager {
        self.manager("objects")
    }

    /// The policy of the model's default manager — what a reverse relation's
    /// manager subclasses when the cascade uses `getattr(instance, accessor)`
    /// without `manager=`.
    #[must_use]
    pub fn default_policy(&self) -> Manager {
        Manager::from_class(&self.default_manager)
    }
}

#[derive(Debug, Deserialize)]
struct RawGraph {
    registry_order: Vec<String>,
    models: Vec<Model>,
}

/// The parsed graph.
pub struct Graph {
    by_label: BTreeMap<String, Model>,
    registry_order: Vec<String>,
}

impl Graph {
    /// The model for a Django label (`db.issue`), case-insensitively like
    /// `apps.get_model`.
    #[must_use]
    pub fn model(&self, label: &str) -> Option<&Model> {
        self.by_label.get(&label.to_lowercase())
    }

    /// `apps.get_models()`' order, restricted to Plane's apps.
    #[must_use]
    pub fn registry_order(&self) -> &[String] {
        &self.registry_order
    }

    /// Every model, in registry order.
    pub fn registry_models(&self) -> impl Iterator<Item = &Model> {
        self.registry_order
            .iter()
            .filter_map(|label| self.by_label.get(label))
    }
}

fn graph() -> &'static Graph {
    static GRAPH: LazyLock<Graph> = LazyLock::new(|| {
        let raw: RawGraph =
            serde_json::from_str(GRAPH_JSON).expect("model_graph.json is embedded and valid");
        let by_label = raw
            .models
            .into_iter()
            .map(|model| (model.label.clone(), model))
            .collect();
        Graph {
            by_label,
            registry_order: raw.registry_order,
        }
    });
    &GRAPH
}

/// The embedded graph.
#[must_use]
pub fn models() -> &'static Graph {
    graph()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graph_has_the_pinned_django_shape() {
        let graph = models();
        let issue = graph.model("db.issue").expect("db.issue");
        assert_eq!(issue.table, "issues");
        assert!(issue.has_deleted_at);
        assert_eq!(issue.objects(), Manager::SoftDelete);
        let activity = issue
            .children
            .iter()
            .find(|child| child.model == "db.issueactivity" && child.field == "issue")
            .expect("issue_activities.issue");
        assert_eq!(activity.on_delete, "DO_NOTHING");
        assert!(activity.null);

        let state = graph.model("db.state").expect("db.state");
        assert_eq!(state.objects(), Manager::State);
        assert_eq!(state.manager("triage_objects"), Manager::TriageState);

        let workspace = graph.model("db.workspace").expect("db.workspace");
        assert!(workspace.children.iter().any(|child| {
            child.model == "db.project"
                && child.column == "workspace_id"
                && child.on_delete == "CASCADE"
        }));
    }

    #[test]
    fn registry_order_is_djangos_not_alphabetical() {
        let order = models().registry_order();
        let analytics = order
            .iter()
            .position(|label| label == "db.analyticview")
            .expect("analyticview");
        let project = order
            .iter()
            .position(|label| label == "db.project")
            .expect("project");
        assert!(analytics < project);
    }

    #[test]
    fn every_child_names_a_known_model() {
        for model in models().registry_models() {
            for child in &model.children {
                assert!(
                    models().model(&child.model).is_some(),
                    "{} references unknown child {}",
                    model.label,
                    child.model
                );
            }
        }
    }
}
