// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated metadata for the database tests.
//!
//! The database tests in `crates/plane-db/tests/models.rs` need the
//! table, columns, Postgres types, nullability, manager predicates and
//! a decoder for every model. Keeping that in typed Rust (rather than
//! parsing `models.json` at test time) lets the compiler check every
//! reference; the JSON file remains the interchange format.

use super::*;

use tokio_postgres::Row;

use crate::catalog::{ArrayElement, PgType};

/// A generated row decoder (`Model::from_row` with the value dropped).
pub type Decode = fn(&Row, usize) -> Result<(), Error>;

/// One column of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnMeta {
    /// Django field name (`created_by`).
    pub field: &'static str,
    /// Database column name (`created_by_id`).
    pub column: &'static str,
    pub pg_type: PgType,
    pub nullable: bool,
    /// The generated Rust field type (`Option<Uuid>` for a nullable
    /// column); kept so the tests can check the `Option` rule.
    pub rust_type: &'static str,
}

/// One manager of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagerMeta {
    pub name: &'static str,
    /// The generated predicate, with the `{alias}` placeholder.
    pub predicate: &'static str,
}

/// Everything the database tests need to know about a model.
#[derive(Debug, Clone, Copy)]
pub struct ModelMeta {
    pub django_label: &'static str,
    pub django_class: &'static str,
    pub rust_struct: &'static str,
    pub table: &'static str,
    pub module: &'static str,
    pub primary_key: &'static str,
    pub soft_delete: bool,
    pub column_count: usize,
    pub ordering: &'static str,
    pub columns_sql: &'static str,
    pub columns: &'static [ColumnMeta],
    pub managers: &'static [ManagerMeta],
    pub decode: Decode,
}

static ANALYTIC_VIEWS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "query",
        column: "query",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "query_dict",
        column: "query_dict",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static ANALYTIC_VIEWS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: AnalyticView::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: AnalyticView::ALL_OBJECTS,
    },
];

fn decode_analytic_views(row: &Row, start: usize) -> Result<(), Error> {
    AnalyticView::from_row(row, start)?;
    Ok(())
}

static API_ACTIVITY_LOGS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "token_identifier",
        column: "token_identifier",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "path",
        column: "path",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "method",
        column: "method",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "query_params",
        column: "query_params",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "headers",
        column: "headers",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "body",
        column: "body",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "response_code",
        column: "response_code",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "response_body",
        column: "response_body",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "ip_address",
        column: "ip_address",
        pg_type: PgType::Inet,
        nullable: true,
        rust_type: "Option<IpAddr>",
    },
    ColumnMeta {
        field: "user_agent",
        column: "user_agent",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static API_ACTIVITY_LOGS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: APIActivityLog::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: APIActivityLog::ALL_OBJECTS,
    },
];

fn decode_api_activity_logs(row: &Row, start: usize) -> Result<(), Error> {
    APIActivityLog::from_row(row, start)?;
    Ok(())
}

static API_TOKENS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "label",
        column: "label",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "last_used",
        column: "last_used",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "token",
        column: "token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user_type",
        column: "user_type",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "expired_at",
        column: "expired_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
];

static API_TOKENS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: APIToken::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: APIToken::ALL_OBJECTS,
    },
];

fn decode_api_tokens(row: &Row, start: usize) -> Result<(), Error> {
    APIToken::from_row(row, start)?;
    Ok(())
}

static FILE_ASSETS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "attributes",
        column: "attributes",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "asset",
        column: "asset",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "draft_issue",
        column: "draft_issue_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "comment",
        column: "comment_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "page",
        column: "page_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_type",
        column: "entity_type",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "is_deleted",
        column: "is_deleted",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_archived",
        column: "is_archived",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "size",
        column: "size",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "is_uploaded",
        column: "is_uploaded",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "storage_metadata",
        column: "storage_metadata",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
];

static FILE_ASSETS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: FileAsset::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: FileAsset::ALL_OBJECTS,
    },
];

fn decode_file_assets(row: &Row, start: usize) -> Result<(), Error> {
    FileAsset::from_row(row, start)?;
    Ok(())
}

static CYCLES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "start_date",
        column: "start_date",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "end_date",
        column: "end_date",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "progress_snapshot",
        column: "progress_snapshot",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "timezone",
        column: "timezone",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "version",
        column: "version",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
];

static CYCLES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Cycle::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Cycle::ALL_OBJECTS,
    },
];

fn decode_cycles(row: &Row, start: usize) -> Result<(), Error> {
    Cycle::from_row(row, start)?;
    Ok(())
}

static CYCLE_ISSUES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "cycle",
        column: "cycle_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static CYCLE_ISSUES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: CycleIssue::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: CycleIssue::ALL_OBJECTS,
    },
];

fn decode_cycle_issues(row: &Row, start: usize) -> Result<(), Error> {
    CycleIssue::from_row(row, start)?;
    Ok(())
}

static CYCLE_USER_PROPERTIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "cycle",
        column: "cycle_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_filters",
        column: "display_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_properties",
        column: "display_properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static CYCLE_USER_PROPERTIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: CycleUserProperties::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: CycleUserProperties::ALL_OBJECTS,
    },
];

fn decode_cycle_user_properties(row: &Row, start: usize) -> Result<(), Error> {
    CycleUserProperties::from_row(row, start)?;
    Ok(())
}

static DEPLOY_BOARDS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_name",
        column: "entity_name",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "anchor",
        column: "anchor",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_comments_enabled",
        column: "is_comments_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_reactions_enabled",
        column: "is_reactions_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "intake",
        column: "intake_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "is_votes_enabled",
        column: "is_votes_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "is_activity_enabled",
        column: "is_activity_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_disabled",
        column: "is_disabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static DEPLOY_BOARDS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DeployBoard::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DeployBoard::ALL_OBJECTS,
    },
];

fn decode_deploy_boards(row: &Row, start: usize) -> Result<(), Error> {
    DeployBoard::from_row(row, start)?;
    Ok(())
}

static DESCRIPTIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static DESCRIPTIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Description::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Description::ALL_OBJECTS,
    },
];

fn decode_descriptions(row: &Row, start: usize) -> Result<(), Error> {
    Description::from_row(row, start)?;
    Ok(())
}

static DESCRIPTION_VERSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "description",
        column: "description_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static DESCRIPTION_VERSIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DescriptionVersion::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DescriptionVersion::ALL_OBJECTS,
    },
];

fn decode_description_versions(row: &Row, start: usize) -> Result<(), Error> {
    DescriptionVersion::from_row(row, start)?;
    Ok(())
}

static DEVICES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "device_id",
        column: "device_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "device_type",
        column: "device_type",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "push_token",
        column: "push_token",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static DEVICES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Device::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Device::ALL_OBJECTS,
    },
];

fn decode_devices(row: &Row, start: usize) -> Result<(), Error> {
    Device::from_row(row, start)?;
    Ok(())
}

static DEVICE_SESSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "device",
        column: "device_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "session",
        column: "session_id",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "user_agent",
        column: "user_agent",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "ip_address",
        column: "ip_address",
        pg_type: PgType::Inet,
        nullable: true,
        rust_type: "Option<IpAddr>",
    },
    ColumnMeta {
        field: "start_time",
        column: "start_time",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "end_time",
        column: "end_time",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
];

static DEVICE_SESSIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DeviceSession::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DeviceSession::ALL_OBJECTS,
    },
];

fn decode_device_sessions(row: &Row, start: usize) -> Result<(), Error> {
    DeviceSession::from_row(row, start)?;
    Ok(())
}

static DRAFT_ISSUES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "state",
        column: "state_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "estimate_point",
        column: "estimate_point_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "priority",
        column: "priority",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "start_date",
        column: "start_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "target_date",
        column: "target_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "completed_at",
        column: "completed_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "type",
        column: "type_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
];

static DRAFT_ISSUES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DraftIssue::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DraftIssue::ALL_OBJECTS,
    },
];

fn decode_draft_issues(row: &Row, start: usize) -> Result<(), Error> {
    DraftIssue::from_row(row, start)?;
    Ok(())
}

static DRAFT_ISSUE_ASSIGNEES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "draft_issue",
        column: "draft_issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "assignee",
        column: "assignee_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static DRAFT_ISSUE_ASSIGNEES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DraftIssueAssignee::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DraftIssueAssignee::ALL_OBJECTS,
    },
];

fn decode_draft_issue_assignees(row: &Row, start: usize) -> Result<(), Error> {
    DraftIssueAssignee::from_row(row, start)?;
    Ok(())
}

static DRAFT_ISSUE_CYCLES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "draft_issue",
        column: "draft_issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "cycle",
        column: "cycle_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static DRAFT_ISSUE_CYCLES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DraftIssueCycle::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DraftIssueCycle::ALL_OBJECTS,
    },
];

fn decode_draft_issue_cycles(row: &Row, start: usize) -> Result<(), Error> {
    DraftIssueCycle::from_row(row, start)?;
    Ok(())
}

static DRAFT_ISSUE_LABELS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "draft_issue",
        column: "draft_issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "label",
        column: "label_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static DRAFT_ISSUE_LABELS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DraftIssueLabel::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DraftIssueLabel::ALL_OBJECTS,
    },
];

fn decode_draft_issue_labels(row: &Row, start: usize) -> Result<(), Error> {
    DraftIssueLabel::from_row(row, start)?;
    Ok(())
}

static DRAFT_ISSUE_MODULES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "module",
        column: "module_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "draft_issue",
        column: "draft_issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static DRAFT_ISSUE_MODULES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: DraftIssueModule::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: DraftIssueModule::ALL_OBJECTS,
    },
];

fn decode_draft_issue_modules(row: &Row, start: usize) -> Result<(), Error> {
    DraftIssueModule::from_row(row, start)?;
    Ok(())
}

static ESTIMATES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "type",
        column: "type",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_used",
        column: "last_used",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static ESTIMATES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Estimate::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Estimate::ALL_OBJECTS,
    },
];

fn decode_estimates(row: &Row, start: usize) -> Result<(), Error> {
    Estimate::from_row(row, start)?;
    Ok(())
}

static ESTIMATE_POINTS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "estimate",
        column: "estimate_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "key",
        column: "key",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "value",
        column: "value",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static ESTIMATE_POINTS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: EstimatePoint::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: EstimatePoint::ALL_OBJECTS,
    },
];

fn decode_estimate_points(row: &Row, start: usize) -> Result<(), Error> {
    EstimatePoint::from_row(row, start)?;
    Ok(())
}

static EXPORTERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "type",
        column: "type",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
        nullable: true,
        rust_type: "Option<Vec<Uuid>>",
    },
    ColumnMeta {
        field: "provider",
        column: "provider",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "status",
        column: "status",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "reason",
        column: "reason",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "key",
        column: "key",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "token",
        column: "token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "initiated_by",
        column: "initiated_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
];

static EXPORTERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ExporterHistory::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ExporterHistory::ALL_OBJECTS,
    },
];

fn decode_exporters(row: &Row, start: usize) -> Result<(), Error> {
    ExporterHistory::from_row(row, start)?;
    Ok(())
}

static USER_FAVORITES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "entity_type",
        column: "entity_type",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "is_folder",
        column: "is_folder",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "sequence",
        column: "sequence",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
];

static USER_FAVORITES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: UserFavorite::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: UserFavorite::ALL_OBJECTS,
    },
];

fn decode_user_favorites(row: &Row, start: usize) -> Result<(), Error> {
    UserFavorite::from_row(row, start)?;
    Ok(())
}

static IMPORTERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "service",
        column: "service",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "status",
        column: "status",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "initiated_by",
        column: "initiated_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "config",
        column: "config",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "data",
        column: "data",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "token",
        column: "token_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "imported_data",
        column: "imported_data",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
];

static IMPORTERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Importer::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Importer::ALL_OBJECTS,
    },
];

fn decode_importers(row: &Row, start: usize) -> Result<(), Error> {
    Importer::from_row(row, start)?;
    Ok(())
}

static INTAKES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_default",
        column: "is_default",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static INTAKES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Intake::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Intake::ALL_OBJECTS,
    },
];

fn decode_intakes(row: &Row, start: usize) -> Result<(), Error> {
    Intake::from_row(row, start)?;
    Ok(())
}

static INTAKE_ISSUES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "intake",
        column: "intake_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "status",
        column: "status",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "snoozed_till",
        column: "snoozed_till",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "duplicate_to",
        column: "duplicate_to_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "source",
        column: "source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "source_email",
        column: "source_email",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "extra",
        column: "extra",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static INTAKE_ISSUES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IntakeIssue::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IntakeIssue::ALL_OBJECTS,
    },
];

fn decode_intake_issues(row: &Row, start: usize) -> Result<(), Error> {
    IntakeIssue::from_row(row, start)?;
    Ok(())
}

static INTEGRATIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "provider",
        column: "provider",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "network",
        column: "network",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "author",
        column: "author",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "webhook_url",
        column: "webhook_url",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "webhook_secret",
        column: "webhook_secret",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "redirect_url",
        column: "redirect_url",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "verified",
        column: "verified",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "avatar_url",
        column: "avatar_url",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static INTEGRATIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Integration::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Integration::ALL_OBJECTS,
    },
];

fn decode_integrations(row: &Row, start: usize) -> Result<(), Error> {
    Integration::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_INTEGRATIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "integration",
        column: "integration_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "api_token",
        column: "api_token_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "config",
        column: "config",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static WORKSPACE_INTEGRATIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceIntegration::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceIntegration::ALL_OBJECTS,
    },
];

fn decode_workspace_integrations(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceIntegration::from_row(row, start)?;
    Ok(())
}

static GITHUB_COMMENT_SYNCS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "repo_comment_id",
        column: "repo_comment_id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "comment",
        column: "comment_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue_sync",
        column: "issue_sync_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static GITHUB_COMMENT_SYNCS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: GithubCommentSync::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: GithubCommentSync::ALL_OBJECTS,
    },
];

fn decode_github_comment_syncs(row: &Row, start: usize) -> Result<(), Error> {
    GithubCommentSync::from_row(row, start)?;
    Ok(())
}

static GITHUB_ISSUE_SYNCS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "repo_issue_id",
        column: "repo_issue_id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "github_issue_id",
        column: "github_issue_id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "issue_url",
        column: "issue_url",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "repository_sync",
        column: "repository_sync_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static GITHUB_ISSUE_SYNCS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: GithubIssueSync::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: GithubIssueSync::ALL_OBJECTS,
    },
];

fn decode_github_issue_syncs(row: &Row, start: usize) -> Result<(), Error> {
    GithubIssueSync::from_row(row, start)?;
    Ok(())
}

static GITHUB_REPOSITORIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "config",
        column: "config",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "repository_id",
        column: "repository_id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "owner",
        column: "owner",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static GITHUB_REPOSITORIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: GithubRepository::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: GithubRepository::ALL_OBJECTS,
    },
];

fn decode_github_repositories(row: &Row, start: usize) -> Result<(), Error> {
    GithubRepository::from_row(row, start)?;
    Ok(())
}

static GITHUB_REPOSITORY_SYNCS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "repository",
        column: "repository_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "credentials",
        column: "credentials",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace_integration",
        column: "workspace_integration_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "label",
        column: "label_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
];

static GITHUB_REPOSITORY_SYNCS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: GithubRepositorySync::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: GithubRepositorySync::ALL_OBJECTS,
    },
];

fn decode_github_repository_syncs(row: &Row, start: usize) -> Result<(), Error> {
    GithubRepositorySync::from_row(row, start)?;
    Ok(())
}

static SLACK_PROJECT_SYNCS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "access_token",
        column: "access_token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "scopes",
        column: "scopes",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "bot_user_id",
        column: "bot_user_id",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "webhook_url",
        column: "webhook_url",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "data",
        column: "data",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "team_id",
        column: "team_id",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "team_name",
        column: "team_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "workspace_integration",
        column: "workspace_integration_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static SLACK_PROJECT_SYNCS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: SlackProjectSync::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: SlackProjectSync::ALL_OBJECTS,
    },
];

fn decode_slack_project_syncs(row: &Row, start: usize) -> Result<(), Error> {
    SlackProjectSync::from_row(row, start)?;
    Ok(())
}

static COMMENT_REACTIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "comment",
        column: "comment_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "reaction",
        column: "reaction",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
];

static COMMENT_REACTIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: CommentReaction::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: CommentReaction::ALL_OBJECTS,
    },
];

fn decode_comment_reactions(row: &Row, start: usize) -> Result<(), Error> {
    CommentReaction::from_row(row, start)?;
    Ok(())
}

static ISSUES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "state",
        column: "state_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "point",
        column: "point",
        pg_type: PgType::Int4,
        nullable: true,
        rust_type: "Option<i32>",
    },
    ColumnMeta {
        field: "estimate_point",
        column: "estimate_point_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "priority",
        column: "priority",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "start_date",
        column: "start_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "target_date",
        column: "target_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "sequence_id",
        column: "sequence_id",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "completed_at",
        column: "completed_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "is_draft",
        column: "is_draft",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "type",
        column: "type_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
];

static ISSUES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "issue_objects",
        predicate: Issue::ISSUE_OBJECTS,
    },
    ManagerMeta {
        name: "objects",
        predicate: Issue::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Issue::ALL_OBJECTS,
    },
];

fn decode_issues(row: &Row, start: usize) -> Result<(), Error> {
    Issue::from_row(row, start)?;
    Ok(())
}

static ISSUE_ACTIVITIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "verb",
        column: "verb",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "field",
        column: "field",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "old_value",
        column: "old_value",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "new_value",
        column: "new_value",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "comment",
        column: "comment",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "attachments",
        column: "attachments",
        pg_type: PgType::ArrayOf(ArrayElement::Varchar),
        nullable: false,
        rust_type: "Vec<String>",
    },
    ColumnMeta {
        field: "issue_comment",
        column: "issue_comment_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "old_identifier",
        column: "old_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "new_identifier",
        column: "new_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "epoch",
        column: "epoch",
        pg_type: PgType::Float8,
        nullable: true,
        rust_type: "Option<f64>",
    },
];

static ISSUE_ACTIVITIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueActivity::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueActivity::ALL_OBJECTS,
    },
];

fn decode_issue_activities(row: &Row, start: usize) -> Result<(), Error> {
    IssueActivity::from_row(row, start)?;
    Ok(())
}

static ISSUE_ASSIGNEES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "assignee",
        column: "assignee_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_ASSIGNEES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueAssignee::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueAssignee::ALL_OBJECTS,
    },
];

fn decode_issue_assignees(row: &Row, start: usize) -> Result<(), Error> {
    IssueAssignee::from_row(row, start)?;
    Ok(())
}

static ISSUE_ATTACHMENTS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "attributes",
        column: "attributes",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "asset",
        column: "asset",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static ISSUE_ATTACHMENTS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueAttachment::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueAttachment::ALL_OBJECTS,
    },
];

fn decode_issue_attachments(row: &Row, start: usize) -> Result<(), Error> {
    IssueAttachment::from_row(row, start)?;
    Ok(())
}

static ISSUE_BLOCKERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "block",
        column: "block_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "blocked_by",
        column: "blocked_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_BLOCKERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueBlocker::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueBlocker::ALL_OBJECTS,
    },
];

fn decode_issue_blockers(row: &Row, start: usize) -> Result<(), Error> {
    IssueBlocker::from_row(row, start)?;
    Ok(())
}

static ISSUE_COMMENTS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "comment_stripped",
        column: "comment_stripped",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "comment_json",
        column: "comment_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "comment_html",
        column: "comment_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "attachments",
        column: "attachments",
        pg_type: PgType::ArrayOf(ArrayElement::Varchar),
        nullable: false,
        rust_type: "Vec<String>",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "access",
        column: "access",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "edited_at",
        column: "edited_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
];

static ISSUE_COMMENTS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueComment::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueComment::ALL_OBJECTS,
    },
];

fn decode_issue_comments(row: &Row, start: usize) -> Result<(), Error> {
    IssueComment::from_row(row, start)?;
    Ok(())
}

static ISSUE_DESCRIPTION_VERSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "last_saved_at",
        column: "last_saved_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_DESCRIPTION_VERSIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueDescriptionVersion::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueDescriptionVersion::ALL_OBJECTS,
    },
];

fn decode_issue_description_versions(row: &Row, start: usize) -> Result<(), Error> {
    IssueDescriptionVersion::from_row(row, start)?;
    Ok(())
}

static ISSUE_LABELS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "label",
        column: "label_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_LABELS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueLabel::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueLabel::ALL_OBJECTS,
    },
];

fn decode_issue_labels(row: &Row, start: usize) -> Result<(), Error> {
    IssueLabel::from_row(row, start)?;
    Ok(())
}

static ISSUE_LINKS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static ISSUE_LINKS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueLink::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueLink::ALL_OBJECTS,
    },
];

fn decode_issue_links(row: &Row, start: usize) -> Result<(), Error> {
    IssueLink::from_row(row, start)?;
    Ok(())
}

static ISSUE_MENTIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "mention",
        column: "mention_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_MENTIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueMention::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueMention::ALL_OBJECTS,
    },
];

fn decode_issue_mentions(row: &Row, start: usize) -> Result<(), Error> {
    IssueMention::from_row(row, start)?;
    Ok(())
}

static ISSUE_REACTIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "reaction",
        column: "reaction",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
];

static ISSUE_REACTIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueReaction::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueReaction::ALL_OBJECTS,
    },
];

fn decode_issue_reactions(row: &Row, start: usize) -> Result<(), Error> {
    IssueReaction::from_row(row, start)?;
    Ok(())
}

static ISSUE_RELATIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "related_issue",
        column: "related_issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "relation_type",
        column: "relation_type",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static ISSUE_RELATIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueRelation::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueRelation::ALL_OBJECTS,
    },
];

fn decode_issue_relations(row: &Row, start: usize) -> Result<(), Error> {
    IssueRelation::from_row(row, start)?;
    Ok(())
}

static ISSUE_SEQUENCES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "sequence",
        column: "sequence",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "deleted",
        column: "deleted",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static ISSUE_SEQUENCES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueSequence::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueSequence::ALL_OBJECTS,
    },
];

fn decode_issue_sequences(row: &Row, start: usize) -> Result<(), Error> {
    IssueSequence::from_row(row, start)?;
    Ok(())
}

static ISSUE_SUBSCRIBERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "subscriber",
        column: "subscriber_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_SUBSCRIBERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueSubscriber::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueSubscriber::ALL_OBJECTS,
    },
];

fn decode_issue_subscribers(row: &Row, start: usize) -> Result<(), Error> {
    IssueSubscriber::from_row(row, start)?;
    Ok(())
}

static ISSUE_VERSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "parent",
        column: "parent",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "state",
        column: "state",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "estimate_point",
        column: "estimate_point",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "priority",
        column: "priority",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "start_date",
        column: "start_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "target_date",
        column: "target_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "assignees",
        column: "assignees",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
        nullable: false,
        rust_type: "Vec<Uuid>",
    },
    ColumnMeta {
        field: "sequence_id",
        column: "sequence_id",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "labels",
        column: "labels",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
        nullable: false,
        rust_type: "Vec<Uuid>",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "completed_at",
        column: "completed_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "is_draft",
        column: "is_draft",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "type",
        column: "type",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "cycle",
        column: "cycle",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "modules",
        column: "modules",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
        nullable: false,
        rust_type: "Vec<Uuid>",
    },
    ColumnMeta {
        field: "properties",
        column: "properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "meta",
        column: "meta",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "last_saved_at",
        column: "last_saved_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "activity",
        column: "activity_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static ISSUE_VERSIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueVersion::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueVersion::ALL_OBJECTS,
    },
];

fn decode_issue_versions(row: &Row, start: usize) -> Result<(), Error> {
    IssueVersion::from_row(row, start)?;
    Ok(())
}

static ISSUE_VOTES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "vote",
        column: "vote",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
];

static ISSUE_VOTES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueVote::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueVote::ALL_OBJECTS,
    },
];

fn decode_issue_votes(row: &Row, start: usize) -> Result<(), Error> {
    IssueVote::from_row(row, start)?;
    Ok(())
}

static ISSUE_TYPES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "is_epic",
        column: "is_epic",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_default",
        column: "is_default",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "level",
        column: "level",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static ISSUE_TYPES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueType::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueType::ALL_OBJECTS,
    },
];

fn decode_issue_types(row: &Row, start: usize) -> Result<(), Error> {
    IssueType::from_row(row, start)?;
    Ok(())
}

static PROJECT_ISSUE_TYPES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue_type",
        column: "issue_type_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "level",
        column: "level",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "is_default",
        column: "is_default",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static PROJECT_ISSUE_TYPES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectIssueType::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectIssueType::ALL_OBJECTS,
    },
];

fn decode_project_issue_types(row: &Row, start: usize) -> Result<(), Error> {
    ProjectIssueType::from_row(row, start)?;
    Ok(())
}

static LABELS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "color",
        column: "color",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static LABELS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Label::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Label::ALL_OBJECTS,
    },
];

fn decode_labels(row: &Row, start: usize) -> Result<(), Error> {
    Label::from_row(row, start)?;
    Ok(())
}

static MODULES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_text",
        column: "description_text",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "start_date",
        column: "start_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "target_date",
        column: "target_date",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "status",
        column: "status",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "lead",
        column: "lead_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static MODULES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Module::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Module::ALL_OBJECTS,
    },
];

fn decode_modules(row: &Row, start: usize) -> Result<(), Error> {
    Module::from_row(row, start)?;
    Ok(())
}

static MODULE_ISSUES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "module",
        column: "module_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "issue",
        column: "issue_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static MODULE_ISSUES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ModuleIssue::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ModuleIssue::ALL_OBJECTS,
    },
];

fn decode_module_issues(row: &Row, start: usize) -> Result<(), Error> {
    ModuleIssue::from_row(row, start)?;
    Ok(())
}

static MODULE_LINKS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "module",
        column: "module_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static MODULE_LINKS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ModuleLink::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ModuleLink::ALL_OBJECTS,
    },
];

fn decode_module_links(row: &Row, start: usize) -> Result<(), Error> {
    ModuleLink::from_row(row, start)?;
    Ok(())
}

static MODULE_MEMBERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "module",
        column: "module_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "member",
        column: "member_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static MODULE_MEMBERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ModuleMember::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ModuleMember::ALL_OBJECTS,
    },
];

fn decode_module_members(row: &Row, start: usize) -> Result<(), Error> {
    ModuleMember::from_row(row, start)?;
    Ok(())
}

static MODULE_USER_PROPERTIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "module",
        column: "module_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_filters",
        column: "display_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_properties",
        column: "display_properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static MODULE_USER_PROPERTIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ModuleUserProperties::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ModuleUserProperties::ALL_OBJECTS,
    },
];

fn decode_module_user_properties(row: &Row, start: usize) -> Result<(), Error> {
    ModuleUserProperties::from_row(row, start)?;
    Ok(())
}

static EMAIL_NOTIFICATION_LOGS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "receiver",
        column: "receiver_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "triggered_by",
        column: "triggered_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_name",
        column: "entity_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "data",
        column: "data",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "processed_at",
        column: "processed_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "sent_at",
        column: "sent_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "entity",
        column: "entity",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "old_value",
        column: "old_value",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "new_value",
        column: "new_value",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static EMAIL_NOTIFICATION_LOGS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: EmailNotificationLog::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: EmailNotificationLog::ALL_OBJECTS,
    },
];

fn decode_email_notification_logs(row: &Row, start: usize) -> Result<(), Error> {
    EmailNotificationLog::from_row(row, start)?;
    Ok(())
}

static NOTIFICATIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "data",
        column: "data",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_name",
        column: "entity_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "message",
        column: "message",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "message_html",
        column: "message_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "message_stripped",
        column: "message_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "sender",
        column: "sender",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "triggered_by",
        column: "triggered_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "receiver",
        column: "receiver_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "read_at",
        column: "read_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "snoozed_till",
        column: "snoozed_till",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
];

static NOTIFICATIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Notification::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Notification::ALL_OBJECTS,
    },
];

fn decode_notifications(row: &Row, start: usize) -> Result<(), Error> {
    Notification::from_row(row, start)?;
    Ok(())
}

static USER_NOTIFICATION_PREFERENCES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "property_change",
        column: "property_change",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "state_change",
        column: "state_change",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "comment",
        column: "comment",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "mention",
        column: "mention",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "issue_completed",
        column: "issue_completed",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static USER_NOTIFICATION_PREFERENCES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: UserNotificationPreference::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: UserNotificationPreference::ALL_OBJECTS,
    },
];

fn decode_user_notification_preferences(row: &Row, start: usize) -> Result<(), Error> {
    UserNotificationPreference::from_row(row, start)?;
    Ok(())
}

static PAGES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "access",
        column: "access",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "color",
        column: "color",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "parent",
        column: "parent_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Date,
        nullable: true,
        rust_type: "Option<NaiveDate>",
    },
    ColumnMeta {
        field: "is_locked",
        column: "is_locked",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "is_global",
        column: "is_global",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "moved_to_page",
        column: "moved_to_page",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "moved_to_project",
        column: "moved_to_project",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static PAGES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Page::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Page::ALL_OBJECTS,
    },
];

fn decode_pages(row: &Row, start: usize) -> Result<(), Error> {
    Page::from_row(row, start)?;
    Ok(())
}

static PAGE_LABELS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "label",
        column: "label_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "page",
        column: "page_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static PAGE_LABELS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: PageLabel::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: PageLabel::ALL_OBJECTS,
    },
];

fn decode_page_labels(row: &Row, start: usize) -> Result<(), Error> {
    PageLabel::from_row(row, start)?;
    Ok(())
}

static PAGE_LOGS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "transaction",
        column: "transaction",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "page",
        column: "page_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_name",
        column: "entity_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "entity_type",
        column: "entity_type",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static PAGE_LOGS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: PageLog::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: PageLog::ALL_OBJECTS,
    },
];

fn decode_page_logs(row: &Row, start: usize) -> Result<(), Error> {
    PageLog::from_row(row, start)?;
    Ok(())
}

static PAGE_VERSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "page",
        column: "page_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "last_saved_at",
        column: "last_saved_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_json",
        column: "description_json",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sub_pages_data",
        column: "sub_pages_data",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static PAGE_VERSIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: PageVersion::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: PageVersion::ALL_OBJECTS,
    },
];

fn decode_page_versions(row: &Row, start: usize) -> Result<(), Error> {
    PageVersion::from_row(row, start)?;
    Ok(())
}

static PROJECT_PAGES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "page",
        column: "page_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static PROJECT_PAGES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectPage::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectPage::ALL_OBJECTS,
    },
];

fn decode_project_pages(row: &Row, start: usize) -> Result<(), Error> {
    ProjectPage::from_row(row, start)?;
    Ok(())
}

static PROJECTS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_text",
        column: "description_text",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "network",
        column: "network",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "identifier",
        column: "identifier",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "default_assignee",
        column: "default_assignee_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "project_lead",
        column: "project_lead_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "emoji",
        column: "emoji",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "icon_prop",
        column: "icon_prop",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "module_view",
        column: "module_view",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "cycle_view",
        column: "cycle_view",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "issue_views_view",
        column: "issue_views_view",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "page_view",
        column: "page_view",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "intake_view",
        column: "intake_view",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_time_tracking_enabled",
        column: "is_time_tracking_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_issue_type_enabled",
        column: "is_issue_type_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "guest_view_all_features",
        column: "guest_view_all_features",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "cover_image",
        column: "cover_image",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "cover_image_asset",
        column: "cover_image_asset_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "estimate",
        column: "estimate_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "archive_in",
        column: "archive_in",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "close_in",
        column: "close_in",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "default_state",
        column: "default_state_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "timezone",
        column: "timezone",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static PROJECTS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Project::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Project::ALL_OBJECTS,
    },
];

fn decode_projects(row: &Row, start: usize) -> Result<(), Error> {
    Project::from_row(row, start)?;
    Ok(())
}

static PROJECT_DEPLOY_BOARDS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "anchor",
        column: "anchor",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "comments",
        column: "comments",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "reactions",
        column: "reactions",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "intake",
        column: "intake_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "votes",
        column: "votes",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "views",
        column: "views",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static PROJECT_DEPLOY_BOARDS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectDeployBoard::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectDeployBoard::ALL_OBJECTS,
    },
];

fn decode_project_deploy_boards(row: &Row, start: usize) -> Result<(), Error> {
    ProjectDeployBoard::from_row(row, start)?;
    Ok(())
}

static PROJECT_IDENTIFIERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static PROJECT_IDENTIFIERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectIdentifier::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectIdentifier::ALL_OBJECTS,
    },
];

fn decode_project_identifiers(row: &Row, start: usize) -> Result<(), Error> {
    ProjectIdentifier::from_row(row, start)?;
    Ok(())
}

static PROJECT_MEMBERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "member",
        column: "member_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "comment",
        column: "comment",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "default_props",
        column: "default_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "preferences",
        column: "preferences",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static PROJECT_MEMBERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectMember::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectMember::ALL_OBJECTS,
    },
];

fn decode_project_members(row: &Row, start: usize) -> Result<(), Error> {
    ProjectMember::from_row(row, start)?;
    Ok(())
}

static PROJECT_MEMBER_INVITES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "email",
        column: "email",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "accepted",
        column: "accepted",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "token",
        column: "token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "message",
        column: "message",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "responded_at",
        column: "responded_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
];

static PROJECT_MEMBER_INVITES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectMemberInvite::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectMemberInvite::ALL_OBJECTS,
    },
];

fn decode_project_member_invites(row: &Row, start: usize) -> Result<(), Error> {
    ProjectMemberInvite::from_row(row, start)?;
    Ok(())
}

static PROJECT_PUBLIC_MEMBERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "member",
        column: "member_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static PROJECT_PUBLIC_MEMBERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectPublicMember::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectPublicMember::ALL_OBJECTS,
    },
];

fn decode_project_public_members(row: &Row, start: usize) -> Result<(), Error> {
    ProjectPublicMember::from_row(row, start)?;
    Ok(())
}

static PROJECT_USER_PROPERTIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_filters",
        column: "display_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_properties",
        column: "display_properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "preferences",
        column: "preferences",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
];

static PROJECT_USER_PROPERTIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectUserProperty::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectUserProperty::ALL_OBJECTS,
    },
];

fn decode_project_user_properties(row: &Row, start: usize) -> Result<(), Error> {
    ProjectUserProperty::from_row(row, start)?;
    Ok(())
}

static USER_RECENT_VISITS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_identifier",
        column: "entity_identifier",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "entity_name",
        column: "entity_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "visited_at",
        column: "visited_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
];

static USER_RECENT_VISITS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: UserRecentVisit::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: UserRecentVisit::ALL_OBJECTS,
    },
];

fn decode_user_recent_visits(row: &Row, start: usize) -> Result<(), Error> {
    UserRecentVisit::from_row(row, start)?;
    Ok(())
}

static SESSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "session_data",
        column: "session_data",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "expire_date",
        column: "expire_date",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "device_info",
        column: "device_info",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "session_key",
        column: "session_key",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "user_id",
        column: "user_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static SESSIONS_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: Session::OBJECTS,
}];

fn decode_sessions(row: &Row, start: usize) -> Result<(), Error> {
    Session::from_row(row, start)?;
    Ok(())
}

static SOCIAL_LOGIN_CONNECTIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "medium",
        column: "medium",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_login_at",
        column: "last_login_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "last_received_at",
        column: "last_received_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "token_data",
        column: "token_data",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "extra_data",
        column: "extra_data",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
];

static SOCIAL_LOGIN_CONNECTIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: SocialLoginConnection::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: SocialLoginConnection::ALL_OBJECTS,
    },
];

fn decode_social_login_connections(row: &Row, start: usize) -> Result<(), Error> {
    SocialLoginConnection::from_row(row, start)?;
    Ok(())
}

static STATES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "color",
        column: "color",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "slug",
        column: "slug",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "sequence",
        column: "sequence",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "group",
        column: "group",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_triage",
        column: "is_triage",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "default",
        column: "default",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "external_source",
        column: "external_source",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "external_id",
        column: "external_id",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
];

static STATES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: State::OBJECTS,
    },
    ManagerMeta {
        name: "all_state_objects",
        predicate: State::ALL_STATE_OBJECTS,
    },
    ManagerMeta {
        name: "triage_objects",
        predicate: State::TRIAGE_OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: State::ALL_OBJECTS,
    },
];

fn decode_states(row: &Row, start: usize) -> Result<(), Error> {
    State::from_row(row, start)?;
    Ok(())
}

static STICKIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "description_html",
        column: "description_html",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description_stripped",
        column: "description_stripped",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "description_binary",
        column: "description_binary",
        pg_type: PgType::Bytea,
        nullable: true,
        rust_type: "Option<Vec<u8>>",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "color",
        column: "color",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "background_color",
        column: "background_color",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "owner",
        column: "owner_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
];

static STICKIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Sticky::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Sticky::ALL_OBJECTS,
    },
];

fn decode_stickies(row: &Row, start: usize) -> Result<(), Error> {
    Sticky::from_row(row, start)?;
    Ok(())
}

static ACCOUNTS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "provider_account_id",
        column: "provider_account_id",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "provider",
        column: "provider",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "access_token",
        column: "access_token",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "access_token_expired_at",
        column: "access_token_expired_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "refresh_token",
        column: "refresh_token",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "refresh_token_expired_at",
        column: "refresh_token_expired_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "last_connected_at",
        column: "last_connected_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "id_token",
        column: "id_token",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static ACCOUNTS_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: Account::OBJECTS,
}];

fn decode_accounts(row: &Row, start: usize) -> Result<(), Error> {
    Account::from_row(row, start)?;
    Ok(())
}

static PROFILES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "theme",
        column: "theme",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "is_app_rail_docked",
        column: "is_app_rail_docked",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_tour_completed",
        column: "is_tour_completed",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "onboarding_step",
        column: "onboarding_step",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "use_case",
        column: "use_case",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "is_onboarded",
        column: "is_onboarded",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "last_workspace_id",
        column: "last_workspace_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "billing_address_country",
        column: "billing_address_country",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "billing_address",
        column: "billing_address",
        pg_type: PgType::Jsonb,
        nullable: true,
        rust_type: "Option<JsonText>",
    },
    ColumnMeta {
        field: "has_billing_address",
        column: "has_billing_address",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "company_name",
        column: "company_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "notification_view_mode",
        column: "notification_view_mode",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_smooth_cursor_enabled",
        column: "is_smooth_cursor_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_mobile_onboarded",
        column: "is_mobile_onboarded",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "mobile_onboarding_step",
        column: "mobile_onboarding_step",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "mobile_timezone_auto_set",
        column: "mobile_timezone_auto_set",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "language",
        column: "language",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "start_of_the_week",
        column: "start_of_the_week",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "goals",
        column: "goals",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "background_color",
        column: "background_color",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_navigation_tour_completed",
        column: "is_navigation_tour_completed",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "has_marketing_email_consent",
        column: "has_marketing_email_consent",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_subscribed_to_changelog",
        column: "is_subscribed_to_changelog",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "product_tour",
        column: "product_tour",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static PROFILES_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: Profile::OBJECTS,
}];

fn decode_profiles(row: &Row, start: usize) -> Result<(), Error> {
    Profile::from_row(row, start)?;
    Ok(())
}

static USERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "password",
        column: "password",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_login",
        column: "last_login",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "username",
        column: "username",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "mobile_number",
        column: "mobile_number",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "email",
        column: "email",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "display_name",
        column: "display_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "first_name",
        column: "first_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_name",
        column: "last_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "avatar",
        column: "avatar",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "avatar_asset",
        column: "avatar_asset_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "cover_image",
        column: "cover_image",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "cover_image_asset",
        column: "cover_image_asset_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "date_joined",
        column: "date_joined",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "last_location",
        column: "last_location",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "created_location",
        column: "created_location",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_superuser",
        column: "is_superuser",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_managed",
        column: "is_managed",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_password_expired",
        column: "is_password_expired",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_staff",
        column: "is_staff",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_email_verified",
        column: "is_email_verified",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_password_autoset",
        column: "is_password_autoset",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_password_reset_required",
        column: "is_password_reset_required",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "token",
        column: "token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_active",
        column: "last_active",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "last_login_time",
        column: "last_login_time",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "last_logout_time",
        column: "last_logout_time",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "last_login_ip",
        column: "last_login_ip",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_logout_ip",
        column: "last_logout_ip",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_login_medium",
        column: "last_login_medium",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_login_uagent",
        column: "last_login_uagent",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "token_updated_at",
        column: "token_updated_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "is_bot",
        column: "is_bot",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "bot_type",
        column: "bot_type",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "user_timezone",
        column: "user_timezone",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_email_valid",
        column: "is_email_valid",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "masked_at",
        column: "masked_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
];

static USERS_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: User::OBJECTS,
}];

fn decode_users(row: &Row, start: usize) -> Result<(), Error> {
    User::from_row(row, start)?;
    Ok(())
}

static USERS_GROUPS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "group",
        column: "group_id",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
];

static USERS_GROUPS_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: UserGroups::OBJECTS,
}];

fn decode_users_groups(row: &Row, start: usize) -> Result<(), Error> {
    UserGroups::from_row(row, start)?;
    Ok(())
}

static USERS_USER_PERMISSIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Int8,
        nullable: false,
        rust_type: "i64",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "permission",
        column: "permission_id",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
];

static USERS_USER_PERMISSIONS_MANAGERS: &[ManagerMeta] = &[ManagerMeta {
    name: "objects",
    predicate: UserUserPermissions::OBJECTS,
}];

fn decode_users_user_permissions(row: &Row, start: usize) -> Result<(), Error> {
    UserUserPermissions::from_row(row, start)?;
    Ok(())
}

static ISSUE_VIEWS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "query",
        column: "query",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_filters",
        column: "display_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_properties",
        column: "display_properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "access",
        column: "access",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "owned_by",
        column: "owned_by_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "is_locked",
        column: "is_locked",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "archived_at",
        column: "archived_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
];

static ISSUE_VIEWS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: IssueView::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: IssueView::ALL_OBJECTS,
    },
];

fn decode_issue_views(row: &Row, start: usize) -> Result<(), Error> {
    IssueView::from_row(row, start)?;
    Ok(())
}

static PROJECT_WEBHOOKS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "webhook",
        column: "webhook_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static PROJECT_WEBHOOKS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ProjectWebhook::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ProjectWebhook::ALL_OBJECTS,
    },
];

fn decode_project_webhooks(row: &Row, start: usize) -> Result<(), Error> {
    ProjectWebhook::from_row(row, start)?;
    Ok(())
}

static WEBHOOKS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "secret_key",
        column: "secret_key",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "project",
        column: "project",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "issue",
        column: "issue",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "module",
        column: "module",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "cycle",
        column: "cycle",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "issue_comment",
        column: "issue_comment",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_internal",
        column: "is_internal",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "version",
        column: "version",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static WEBHOOKS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Webhook::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Webhook::ALL_OBJECTS,
    },
];

fn decode_webhooks(row: &Row, start: usize) -> Result<(), Error> {
    Webhook::from_row(row, start)?;
    Ok(())
}

static WEBHOOK_LOGS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "webhook",
        column: "webhook",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "event_type",
        column: "event_type",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "request_method",
        column: "request_method",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "request_headers",
        column: "request_headers",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "request_body",
        column: "request_body",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "response_status",
        column: "response_status",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "response_headers",
        column: "response_headers",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "response_body",
        column: "response_body",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "retry_count",
        column: "retry_count",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
];

static WEBHOOK_LOGS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WebhookLog::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WebhookLog::ALL_OBJECTS,
    },
];

fn decode_webhook_logs(row: &Row, start: usize) -> Result<(), Error> {
    WebhookLog::from_row(row, start)?;
    Ok(())
}

static TEAMS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "logo_props",
        column: "logo_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static TEAMS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Team::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Team::ALL_OBJECTS,
    },
];

fn decode_teams(row: &Row, start: usize) -> Result<(), Error> {
    Team::from_row(row, start)?;
    Ok(())
}

static WORKSPACES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "logo",
        column: "logo",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "logo_asset",
        column: "logo_asset_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "owner",
        column: "owner_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "slug",
        column: "slug",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "organization_size",
        column: "organization_size",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "timezone",
        column: "timezone",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "background_color",
        column: "background_color",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static WORKSPACES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Workspace::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Workspace::ALL_OBJECTS,
    },
];

fn decode_workspaces(row: &Row, start: usize) -> Result<(), Error> {
    Workspace::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_HOME_PREFERENCES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "key",
        column: "key",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_enabled",
        column: "is_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "config",
        column: "config",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
];

static WORKSPACE_HOME_PREFERENCES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceHomePreference::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceHomePreference::ALL_OBJECTS,
    },
];

fn decode_workspace_home_preferences(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceHomePreference::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_MEMBERS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "member",
        column: "member_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
    ColumnMeta {
        field: "company_role",
        column: "company_role",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "view_props",
        column: "view_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "default_props",
        column: "default_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "issue_props",
        column: "issue_props",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "is_active",
        column: "is_active",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "getting_started_checklist",
        column: "getting_started_checklist",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "tips",
        column: "tips",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "explored_features",
        column: "explored_features",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static WORKSPACE_MEMBERS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceMember::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceMember::ALL_OBJECTS,
    },
];

fn decode_workspace_members(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceMember::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_MEMBER_INVITES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "email",
        column: "email",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "accepted",
        column: "accepted",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "token",
        column: "token",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "message",
        column: "message",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "responded_at",
        column: "responded_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Int2,
        nullable: false,
        rust_type: "i16",
    },
];

static WORKSPACE_MEMBER_INVITES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceMemberInvite::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceMemberInvite::ALL_OBJECTS,
    },
];

fn decode_workspace_member_invites(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceMemberInvite::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_THEMES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "name",
        column: "name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "actor",
        column: "actor_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "colors",
        column: "colors",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
];

static WORKSPACE_THEMES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceTheme::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceTheme::ALL_OBJECTS,
    },
];

fn decode_workspace_themes(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceTheme::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_USER_LINKS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "project",
        column: "project_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "url",
        column: "url",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "metadata",
        column: "metadata",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "owner",
        column: "owner_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
];

static WORKSPACE_USER_LINKS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceUserLink::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceUserLink::ALL_OBJECTS,
    },
];

fn decode_workspace_user_links(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceUserLink::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_USER_PREFERENCES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "key",
        column: "key",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_pinned",
        column: "is_pinned",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "sort_order",
        column: "sort_order",
        pg_type: PgType::Float8,
        nullable: false,
        rust_type: "f64",
    },
];

static WORKSPACE_USER_PREFERENCES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceUserPreference::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceUserPreference::ALL_OBJECTS,
    },
];

fn decode_workspace_user_preferences(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceUserPreference::from_row(row, start)?;
    Ok(())
}

static WORKSPACE_USER_PROPERTIES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "workspace",
        column: "workspace_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "filters",
        column: "filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_filters",
        column: "display_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "display_properties",
        column: "display_properties",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "rich_filters",
        column: "rich_filters",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "navigation_project_limit",
        column: "navigation_project_limit",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "navigation_control_preference",
        column: "navigation_control_preference",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
];

static WORKSPACE_USER_PROPERTIES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: WorkspaceUserProperties::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: WorkspaceUserProperties::ALL_OBJECTS,
    },
];

fn decode_workspace_user_properties(row: &Row, start: usize) -> Result<(), Error> {
    WorkspaceUserProperties::from_row(row, start)?;
    Ok(())
}

static CHANGELOGS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "title",
        column: "title",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "description",
        column: "description",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "version",
        column: "version",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "tags",
        column: "tags",
        pg_type: PgType::Jsonb,
        nullable: false,
        rust_type: "JsonText",
    },
    ColumnMeta {
        field: "release_date",
        column: "release_date",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "is_release_candidate",
        column: "is_release_candidate",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static CHANGELOGS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: ChangeLog::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: ChangeLog::ALL_OBJECTS,
    },
];

fn decode_changelogs(row: &Row, start: usize) -> Result<(), Error> {
    ChangeLog::from_row(row, start)?;
    Ok(())
}

static INSTANCES_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "instance_name",
        column: "instance_name",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "whitelist_emails",
        column: "whitelist_emails",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "instance_id",
        column: "instance_id",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "current_version",
        column: "current_version",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "latest_version",
        column: "latest_version",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "edition",
        column: "edition",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "domain",
        column: "domain",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "last_checked_at",
        column: "last_checked_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "namespace",
        column: "namespace",
        pg_type: PgType::Varchar,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "is_telemetry_enabled",
        column: "is_telemetry_enabled",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_support_required",
        column: "is_support_required",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_setup_done",
        column: "is_setup_done",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_signup_screen_visited",
        column: "is_signup_screen_visited",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_verified",
        column: "is_verified",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_test",
        column: "is_test",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
    ColumnMeta {
        field: "is_current_version_deprecated",
        column: "is_current_version_deprecated",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static INSTANCES_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: Instance::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: Instance::ALL_OBJECTS,
    },
];

fn decode_instances(row: &Row, start: usize) -> Result<(), Error> {
    Instance::from_row(row, start)?;
    Ok(())
}

static INSTANCE_ADMINS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "user",
        column: "user_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "instance",
        column: "instance_id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "role",
        column: "role",
        pg_type: PgType::Int4,
        nullable: false,
        rust_type: "i32",
    },
    ColumnMeta {
        field: "is_verified",
        column: "is_verified",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static INSTANCE_ADMINS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: InstanceAdmin::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: InstanceAdmin::ALL_OBJECTS,
    },
];

fn decode_instance_admins(row: &Row, start: usize) -> Result<(), Error> {
    InstanceAdmin::from_row(row, start)?;
    Ok(())
}

static INSTANCE_CONFIGURATIONS_COLUMNS: &[ColumnMeta] = &[
    ColumnMeta {
        field: "created_at",
        column: "created_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "updated_at",
        column: "updated_at",
        pg_type: PgType::Timestamptz,
        nullable: false,
        rust_type: "DateTime<Utc>",
    },
    ColumnMeta {
        field: "created_by",
        column: "created_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "updated_by",
        column: "updated_by_id",
        pg_type: PgType::Uuid,
        nullable: true,
        rust_type: "Option<Uuid>",
    },
    ColumnMeta {
        field: "deleted_at",
        column: "deleted_at",
        pg_type: PgType::Timestamptz,
        nullable: true,
        rust_type: "Option<DateTime<Utc>>",
    },
    ColumnMeta {
        field: "id",
        column: "id",
        pg_type: PgType::Uuid,
        nullable: false,
        rust_type: "Uuid",
    },
    ColumnMeta {
        field: "key",
        column: "key",
        pg_type: PgType::Varchar,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "value",
        column: "value",
        pg_type: PgType::Text,
        nullable: true,
        rust_type: "Option<String>",
    },
    ColumnMeta {
        field: "category",
        column: "category",
        pg_type: PgType::Text,
        nullable: false,
        rust_type: "String",
    },
    ColumnMeta {
        field: "is_encrypted",
        column: "is_encrypted",
        pg_type: PgType::Bool,
        nullable: false,
        rust_type: "bool",
    },
];

static INSTANCE_CONFIGURATIONS_MANAGERS: &[ManagerMeta] = &[
    ManagerMeta {
        name: "objects",
        predicate: InstanceConfiguration::OBJECTS,
    },
    ManagerMeta {
        name: "all_objects",
        predicate: InstanceConfiguration::ALL_OBJECTS,
    },
];

fn decode_instance_configurations(row: &Row, start: usize) -> Result<(), Error> {
    InstanceConfiguration::from_row(row, start)?;
    Ok(())
}

/// Every generated model, in `models.json` order.
pub static MODELS: &[ModelMeta] = &[
    ModelMeta {
        django_label: "db.AnalyticView",
        django_class: "AnalyticView",
        rust_struct: "AnalyticView",
        table: "analytic_views",
        module: "analytic",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: AnalyticView::ORDER_BY,
        columns_sql: AnalyticView::COLUMNS,
        columns: ANALYTIC_VIEWS_COLUMNS,
        managers: ANALYTIC_VIEWS_MANAGERS,
        decode: decode_analytic_views,
    },
    ModelMeta {
        django_label: "db.APIActivityLog",
        django_class: "APIActivityLog",
        rust_struct: "APIActivityLog",
        table: "api_activity_logs",
        module: "api",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: APIActivityLog::ORDER_BY,
        columns_sql: APIActivityLog::COLUMNS,
        columns: API_ACTIVITY_LOGS_COLUMNS,
        managers: API_ACTIVITY_LOGS_MANAGERS,
        decode: decode_api_activity_logs,
    },
    ModelMeta {
        django_label: "db.APIToken",
        django_class: "APIToken",
        rust_struct: "APIToken",
        table: "api_tokens",
        module: "api",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: APIToken::ORDER_BY,
        columns_sql: APIToken::COLUMNS,
        columns: API_TOKENS_COLUMNS,
        managers: API_TOKENS_MANAGERS,
        decode: decode_api_tokens,
    },
    ModelMeta {
        django_label: "db.FileAsset",
        django_class: "FileAsset",
        rust_struct: "FileAsset",
        table: "file_assets",
        module: "asset",
        primary_key: "id",
        soft_delete: true,
        column_count: 24,
        ordering: FileAsset::ORDER_BY,
        columns_sql: FileAsset::COLUMNS,
        columns: FILE_ASSETS_COLUMNS,
        managers: FILE_ASSETS_MANAGERS,
        decode: decode_file_assets,
    },
    ModelMeta {
        django_label: "db.Cycle",
        django_class: "Cycle",
        rust_struct: "Cycle",
        table: "cycles",
        module: "cycle",
        primary_key: "id",
        soft_delete: true,
        column_count: 22,
        ordering: Cycle::ORDER_BY,
        columns_sql: Cycle::COLUMNS,
        columns: CYCLES_COLUMNS,
        managers: CYCLES_MANAGERS,
        decode: decode_cycles,
    },
    ModelMeta {
        django_label: "db.CycleIssue",
        django_class: "CycleIssue",
        rust_struct: "CycleIssue",
        table: "cycle_issues",
        module: "cycle",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: CycleIssue::ORDER_BY,
        columns_sql: CycleIssue::COLUMNS,
        columns: CYCLE_ISSUES_COLUMNS,
        managers: CYCLE_ISSUES_MANAGERS,
        decode: decode_cycle_issues,
    },
    ModelMeta {
        django_label: "db.CycleUserProperties",
        django_class: "CycleUserProperties",
        rust_struct: "CycleUserProperties",
        table: "cycle_user_properties",
        module: "cycle",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: CycleUserProperties::ORDER_BY,
        columns_sql: CycleUserProperties::COLUMNS,
        columns: CYCLE_USER_PROPERTIES_COLUMNS,
        managers: CYCLE_USER_PROPERTIES_MANAGERS,
        decode: decode_cycle_user_properties,
    },
    ModelMeta {
        django_label: "db.DeployBoard",
        django_class: "DeployBoard",
        rust_struct: "DeployBoard",
        table: "deploy_boards",
        module: "deploy_board",
        primary_key: "id",
        soft_delete: true,
        column_count: 18,
        ordering: DeployBoard::ORDER_BY,
        columns_sql: DeployBoard::COLUMNS,
        columns: DEPLOY_BOARDS_COLUMNS,
        managers: DEPLOY_BOARDS_MANAGERS,
        decode: decode_deploy_boards,
    },
    ModelMeta {
        django_label: "db.Description",
        django_class: "Description",
        rust_struct: "Description",
        table: "descriptions",
        module: "description",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: Description::ORDER_BY,
        columns_sql: Description::COLUMNS,
        columns: DESCRIPTIONS_COLUMNS,
        managers: DESCRIPTIONS_MANAGERS,
        decode: decode_descriptions,
    },
    ModelMeta {
        django_label: "db.DescriptionVersion",
        django_class: "DescriptionVersion",
        rust_struct: "DescriptionVersion",
        table: "description_versions",
        module: "description",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: DescriptionVersion::ORDER_BY,
        columns_sql: DescriptionVersion::COLUMNS,
        columns: DESCRIPTION_VERSIONS_COLUMNS,
        managers: DESCRIPTION_VERSIONS_MANAGERS,
        decode: decode_description_versions,
    },
    ModelMeta {
        django_label: "db.Device",
        django_class: "Device",
        rust_struct: "Device",
        table: "devices",
        module: "device",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: Device::ORDER_BY,
        columns_sql: Device::COLUMNS,
        columns: DEVICES_COLUMNS,
        managers: DEVICES_MANAGERS,
        decode: decode_devices,
    },
    ModelMeta {
        django_label: "db.DeviceSession",
        django_class: "DeviceSession",
        rust_struct: "DeviceSession",
        table: "device_sessions",
        module: "device",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: DeviceSession::ORDER_BY,
        columns_sql: DeviceSession::COLUMNS,
        columns: DEVICE_SESSIONS_COLUMNS,
        managers: DEVICE_SESSIONS_MANAGERS,
        decode: decode_device_sessions,
    },
    ModelMeta {
        django_label: "db.DraftIssue",
        django_class: "DraftIssue",
        rust_struct: "DraftIssue",
        table: "draft_issues",
        module: "draft",
        primary_key: "id",
        soft_delete: true,
        column_count: 24,
        ordering: DraftIssue::ORDER_BY,
        columns_sql: DraftIssue::COLUMNS,
        columns: DRAFT_ISSUES_COLUMNS,
        managers: DRAFT_ISSUES_MANAGERS,
        decode: decode_draft_issues,
    },
    ModelMeta {
        django_label: "db.DraftIssueAssignee",
        django_class: "DraftIssueAssignee",
        rust_struct: "DraftIssueAssignee",
        table: "draft_issue_assignees",
        module: "draft",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: DraftIssueAssignee::ORDER_BY,
        columns_sql: DraftIssueAssignee::COLUMNS,
        columns: DRAFT_ISSUE_ASSIGNEES_COLUMNS,
        managers: DRAFT_ISSUE_ASSIGNEES_MANAGERS,
        decode: decode_draft_issue_assignees,
    },
    ModelMeta {
        django_label: "db.DraftIssueCycle",
        django_class: "DraftIssueCycle",
        rust_struct: "DraftIssueCycle",
        table: "draft_issue_cycles",
        module: "draft",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: DraftIssueCycle::ORDER_BY,
        columns_sql: DraftIssueCycle::COLUMNS,
        columns: DRAFT_ISSUE_CYCLES_COLUMNS,
        managers: DRAFT_ISSUE_CYCLES_MANAGERS,
        decode: decode_draft_issue_cycles,
    },
    ModelMeta {
        django_label: "db.DraftIssueLabel",
        django_class: "DraftIssueLabel",
        rust_struct: "DraftIssueLabel",
        table: "draft_issue_labels",
        module: "draft",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: DraftIssueLabel::ORDER_BY,
        columns_sql: DraftIssueLabel::COLUMNS,
        columns: DRAFT_ISSUE_LABELS_COLUMNS,
        managers: DRAFT_ISSUE_LABELS_MANAGERS,
        decode: decode_draft_issue_labels,
    },
    ModelMeta {
        django_label: "db.DraftIssueModule",
        django_class: "DraftIssueModule",
        rust_struct: "DraftIssueModule",
        table: "draft_issue_modules",
        module: "draft",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: DraftIssueModule::ORDER_BY,
        columns_sql: DraftIssueModule::COLUMNS,
        columns: DRAFT_ISSUE_MODULES_COLUMNS,
        managers: DRAFT_ISSUE_MODULES_MANAGERS,
        decode: decode_draft_issue_modules,
    },
    ModelMeta {
        django_label: "db.Estimate",
        django_class: "Estimate",
        rust_struct: "Estimate",
        table: "estimates",
        module: "estimate",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: Estimate::ORDER_BY,
        columns_sql: Estimate::COLUMNS,
        columns: ESTIMATES_COLUMNS,
        managers: ESTIMATES_MANAGERS,
        decode: decode_estimates,
    },
    ModelMeta {
        django_label: "db.EstimatePoint",
        django_class: "EstimatePoint",
        rust_struct: "EstimatePoint",
        table: "estimate_points",
        module: "estimate",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: EstimatePoint::ORDER_BY,
        columns_sql: EstimatePoint::COLUMNS,
        columns: ESTIMATE_POINTS_COLUMNS,
        managers: ESTIMATE_POINTS_MANAGERS,
        decode: decode_estimate_points,
    },
    ModelMeta {
        django_label: "db.ExporterHistory",
        django_class: "ExporterHistory",
        rust_struct: "ExporterHistory",
        table: "exporters",
        module: "exporter",
        primary_key: "id",
        soft_delete: true,
        column_count: 19,
        ordering: ExporterHistory::ORDER_BY,
        columns_sql: ExporterHistory::COLUMNS,
        columns: EXPORTERS_COLUMNS,
        managers: EXPORTERS_MANAGERS,
        decode: decode_exporters,
    },
    ModelMeta {
        django_label: "db.UserFavorite",
        django_class: "UserFavorite",
        rust_struct: "UserFavorite",
        table: "user_favorites",
        module: "favorite",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: UserFavorite::ORDER_BY,
        columns_sql: UserFavorite::COLUMNS,
        columns: USER_FAVORITES_COLUMNS,
        managers: USER_FAVORITES_MANAGERS,
        decode: decode_user_favorites,
    },
    ModelMeta {
        django_label: "db.Importer",
        django_class: "Importer",
        rust_struct: "Importer",
        table: "importers",
        module: "importer",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: Importer::ORDER_BY,
        columns_sql: Importer::COLUMNS,
        columns: IMPORTERS_COLUMNS,
        managers: IMPORTERS_MANAGERS,
        decode: decode_importers,
    },
    ModelMeta {
        django_label: "db.Intake",
        django_class: "Intake",
        rust_struct: "Intake",
        table: "intakes",
        module: "intake",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: Intake::ORDER_BY,
        columns_sql: Intake::COLUMNS,
        columns: INTAKES_COLUMNS,
        managers: INTAKES_MANAGERS,
        decode: decode_intakes,
    },
    ModelMeta {
        django_label: "db.IntakeIssue",
        django_class: "IntakeIssue",
        rust_struct: "IntakeIssue",
        table: "intake_issues",
        module: "intake",
        primary_key: "id",
        soft_delete: true,
        column_count: 18,
        ordering: IntakeIssue::ORDER_BY,
        columns_sql: IntakeIssue::COLUMNS,
        columns: INTAKE_ISSUES_COLUMNS,
        managers: INTAKE_ISSUES_MANAGERS,
        decode: decode_intake_issues,
    },
    ModelMeta {
        django_label: "db.Integration",
        django_class: "Integration",
        rust_struct: "Integration",
        table: "integrations",
        module: "integration/base",
        primary_key: "id",
        soft_delete: true,
        column_count: 17,
        ordering: Integration::ORDER_BY,
        columns_sql: Integration::COLUMNS,
        columns: INTEGRATIONS_COLUMNS,
        managers: INTEGRATIONS_MANAGERS,
        decode: decode_integrations,
    },
    ModelMeta {
        django_label: "db.WorkspaceIntegration",
        django_class: "WorkspaceIntegration",
        rust_struct: "WorkspaceIntegration",
        table: "workspace_integrations",
        module: "integration/base",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: WorkspaceIntegration::ORDER_BY,
        columns_sql: WorkspaceIntegration::COLUMNS,
        columns: WORKSPACE_INTEGRATIONS_COLUMNS,
        managers: WORKSPACE_INTEGRATIONS_MANAGERS,
        decode: decode_workspace_integrations,
    },
    ModelMeta {
        django_label: "db.GithubCommentSync",
        django_class: "GithubCommentSync",
        rust_struct: "GithubCommentSync",
        table: "github_comment_syncs",
        module: "integration/github",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: GithubCommentSync::ORDER_BY,
        columns_sql: GithubCommentSync::COLUMNS,
        columns: GITHUB_COMMENT_SYNCS_COLUMNS,
        managers: GITHUB_COMMENT_SYNCS_MANAGERS,
        decode: decode_github_comment_syncs,
    },
    ModelMeta {
        django_label: "db.GithubIssueSync",
        django_class: "GithubIssueSync",
        rust_struct: "GithubIssueSync",
        table: "github_issue_syncs",
        module: "integration/github",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: GithubIssueSync::ORDER_BY,
        columns_sql: GithubIssueSync::COLUMNS,
        columns: GITHUB_ISSUE_SYNCS_COLUMNS,
        managers: GITHUB_ISSUE_SYNCS_MANAGERS,
        decode: decode_github_issue_syncs,
    },
    ModelMeta {
        django_label: "db.GithubRepository",
        django_class: "GithubRepository",
        rust_struct: "GithubRepository",
        table: "github_repositories",
        module: "integration/github",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: GithubRepository::ORDER_BY,
        columns_sql: GithubRepository::COLUMNS,
        columns: GITHUB_REPOSITORIES_COLUMNS,
        managers: GITHUB_REPOSITORIES_MANAGERS,
        decode: decode_github_repositories,
    },
    ModelMeta {
        django_label: "db.GithubRepositorySync",
        django_class: "GithubRepositorySync",
        rust_struct: "GithubRepositorySync",
        table: "github_repository_syncs",
        module: "integration/github",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: GithubRepositorySync::ORDER_BY,
        columns_sql: GithubRepositorySync::COLUMNS,
        columns: GITHUB_REPOSITORY_SYNCS_COLUMNS,
        managers: GITHUB_REPOSITORY_SYNCS_MANAGERS,
        decode: decode_github_repository_syncs,
    },
    ModelMeta {
        django_label: "db.SlackProjectSync",
        django_class: "SlackProjectSync",
        rust_struct: "SlackProjectSync",
        table: "slack_project_syncs",
        module: "integration/slack",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: SlackProjectSync::ORDER_BY,
        columns_sql: SlackProjectSync::COLUMNS,
        columns: SLACK_PROJECT_SYNCS_COLUMNS,
        managers: SLACK_PROJECT_SYNCS_MANAGERS,
        decode: decode_slack_project_syncs,
    },
    ModelMeta {
        django_label: "db.CommentReaction",
        django_class: "CommentReaction",
        rust_struct: "CommentReaction",
        table: "comment_reactions",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: CommentReaction::ORDER_BY,
        columns_sql: CommentReaction::COLUMNS,
        columns: COMMENT_REACTIONS_COLUMNS,
        managers: COMMENT_REACTIONS_MANAGERS,
        decode: decode_comment_reactions,
    },
    ModelMeta {
        django_label: "db.Issue",
        django_class: "Issue",
        rust_struct: "Issue",
        table: "issues",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 28,
        ordering: Issue::ORDER_BY,
        columns_sql: Issue::COLUMNS,
        columns: ISSUES_COLUMNS,
        managers: ISSUES_MANAGERS,
        decode: decode_issues,
    },
    ModelMeta {
        django_label: "db.IssueActivity",
        django_class: "IssueActivity",
        rust_struct: "IssueActivity",
        table: "issue_activities",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 20,
        ordering: IssueActivity::ORDER_BY,
        columns_sql: IssueActivity::COLUMNS,
        columns: ISSUE_ACTIVITIES_COLUMNS,
        managers: ISSUE_ACTIVITIES_MANAGERS,
        decode: decode_issue_activities,
    },
    ModelMeta {
        django_label: "db.IssueAssignee",
        django_class: "IssueAssignee",
        rust_struct: "IssueAssignee",
        table: "issue_assignees",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: IssueAssignee::ORDER_BY,
        columns_sql: IssueAssignee::COLUMNS,
        columns: ISSUE_ASSIGNEES_COLUMNS,
        managers: ISSUE_ASSIGNEES_MANAGERS,
        decode: decode_issue_assignees,
    },
    ModelMeta {
        django_label: "db.IssueAttachment",
        django_class: "IssueAttachment",
        rust_struct: "IssueAttachment",
        table: "issue_attachments",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: IssueAttachment::ORDER_BY,
        columns_sql: IssueAttachment::COLUMNS,
        columns: ISSUE_ATTACHMENTS_COLUMNS,
        managers: ISSUE_ATTACHMENTS_MANAGERS,
        decode: decode_issue_attachments,
    },
    ModelMeta {
        django_label: "db.IssueBlocker",
        django_class: "IssueBlocker",
        rust_struct: "IssueBlocker",
        table: "issue_blockers",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: IssueBlocker::ORDER_BY,
        columns_sql: IssueBlocker::COLUMNS,
        columns: ISSUE_BLOCKERS_COLUMNS,
        managers: ISSUE_BLOCKERS_MANAGERS,
        decode: decode_issue_blockers,
    },
    ModelMeta {
        django_label: "db.IssueComment",
        django_class: "IssueComment",
        rust_struct: "IssueComment",
        table: "issue_comments",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 20,
        ordering: IssueComment::ORDER_BY,
        columns_sql: IssueComment::COLUMNS,
        columns: ISSUE_COMMENTS_COLUMNS,
        managers: ISSUE_COMMENTS_MANAGERS,
        decode: decode_issue_comments,
    },
    ModelMeta {
        django_label: "db.IssueDescriptionVersion",
        django_class: "IssueDescriptionVersion",
        rust_struct: "IssueDescriptionVersion",
        table: "issue_description_versions",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: IssueDescriptionVersion::ORDER_BY,
        columns_sql: IssueDescriptionVersion::COLUMNS,
        columns: ISSUE_DESCRIPTION_VERSIONS_COLUMNS,
        managers: ISSUE_DESCRIPTION_VERSIONS_MANAGERS,
        decode: decode_issue_description_versions,
    },
    ModelMeta {
        django_label: "db.IssueLabel",
        django_class: "IssueLabel",
        rust_struct: "IssueLabel",
        table: "issue_labels",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: IssueLabel::ORDER_BY,
        columns_sql: IssueLabel::COLUMNS,
        columns: ISSUE_LABELS_COLUMNS,
        managers: ISSUE_LABELS_MANAGERS,
        decode: decode_issue_labels,
    },
    ModelMeta {
        django_label: "db.IssueLink",
        django_class: "IssueLink",
        rust_struct: "IssueLink",
        table: "issue_links",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: IssueLink::ORDER_BY,
        columns_sql: IssueLink::COLUMNS,
        columns: ISSUE_LINKS_COLUMNS,
        managers: ISSUE_LINKS_MANAGERS,
        decode: decode_issue_links,
    },
    ModelMeta {
        django_label: "db.IssueMention",
        django_class: "IssueMention",
        rust_struct: "IssueMention",
        table: "issue_mentions",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: IssueMention::ORDER_BY,
        columns_sql: IssueMention::COLUMNS,
        columns: ISSUE_MENTIONS_COLUMNS,
        managers: ISSUE_MENTIONS_MANAGERS,
        decode: decode_issue_mentions,
    },
    ModelMeta {
        django_label: "db.IssueReaction",
        django_class: "IssueReaction",
        rust_struct: "IssueReaction",
        table: "issue_reactions",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: IssueReaction::ORDER_BY,
        columns_sql: IssueReaction::COLUMNS,
        columns: ISSUE_REACTIONS_COLUMNS,
        managers: ISSUE_REACTIONS_MANAGERS,
        decode: decode_issue_reactions,
    },
    ModelMeta {
        django_label: "db.IssueRelation",
        django_class: "IssueRelation",
        rust_struct: "IssueRelation",
        table: "issue_relations",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: IssueRelation::ORDER_BY,
        columns_sql: IssueRelation::COLUMNS,
        columns: ISSUE_RELATIONS_COLUMNS,
        managers: ISSUE_RELATIONS_MANAGERS,
        decode: decode_issue_relations,
    },
    ModelMeta {
        django_label: "db.IssueSequence",
        django_class: "IssueSequence",
        rust_struct: "IssueSequence",
        table: "issue_sequences",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: IssueSequence::ORDER_BY,
        columns_sql: IssueSequence::COLUMNS,
        columns: ISSUE_SEQUENCES_COLUMNS,
        managers: ISSUE_SEQUENCES_MANAGERS,
        decode: decode_issue_sequences,
    },
    ModelMeta {
        django_label: "db.IssueSubscriber",
        django_class: "IssueSubscriber",
        rust_struct: "IssueSubscriber",
        table: "issue_subscribers",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: IssueSubscriber::ORDER_BY,
        columns_sql: IssueSubscriber::COLUMNS,
        columns: ISSUE_SUBSCRIBERS_COLUMNS,
        managers: ISSUE_SUBSCRIBERS_MANAGERS,
        decode: decode_issue_subscribers,
    },
    ModelMeta {
        django_label: "db.IssueVersion",
        django_class: "IssueVersion",
        rust_struct: "IssueVersion",
        table: "issue_versions",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 33,
        ordering: IssueVersion::ORDER_BY,
        columns_sql: IssueVersion::COLUMNS,
        columns: ISSUE_VERSIONS_COLUMNS,
        managers: ISSUE_VERSIONS_MANAGERS,
        decode: decode_issue_versions,
    },
    ModelMeta {
        django_label: "db.IssueVote",
        django_class: "IssueVote",
        rust_struct: "IssueVote",
        table: "issue_votes",
        module: "issue",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: IssueVote::ORDER_BY,
        columns_sql: IssueVote::COLUMNS,
        columns: ISSUE_VOTES_COLUMNS,
        managers: ISSUE_VOTES_MANAGERS,
        decode: decode_issue_votes,
    },
    ModelMeta {
        django_label: "db.IssueType",
        django_class: "IssueType",
        rust_struct: "IssueType",
        table: "issue_types",
        module: "issue_type",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: IssueType::ORDER_BY,
        columns_sql: IssueType::COLUMNS,
        columns: ISSUE_TYPES_COLUMNS,
        managers: ISSUE_TYPES_MANAGERS,
        decode: decode_issue_types,
    },
    ModelMeta {
        django_label: "db.ProjectIssueType",
        django_class: "ProjectIssueType",
        rust_struct: "ProjectIssueType",
        table: "project_issue_types",
        module: "issue_type",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: ProjectIssueType::ORDER_BY,
        columns_sql: ProjectIssueType::COLUMNS,
        columns: PROJECT_ISSUE_TYPES_COLUMNS,
        managers: PROJECT_ISSUE_TYPES_MANAGERS,
        decode: decode_project_issue_types,
    },
    ModelMeta {
        django_label: "db.Label",
        django_class: "Label",
        rust_struct: "Label",
        table: "labels",
        module: "label",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: Label::ORDER_BY,
        columns_sql: Label::COLUMNS,
        columns: LABELS_COLUMNS,
        managers: LABELS_MANAGERS,
        decode: decode_labels,
    },
    ModelMeta {
        django_label: "db.Module",
        django_class: "Module",
        rust_struct: "Module",
        table: "modules",
        module: "module",
        primary_key: "id",
        soft_delete: true,
        column_count: 22,
        ordering: Module::ORDER_BY,
        columns_sql: Module::COLUMNS,
        columns: MODULES_COLUMNS,
        managers: MODULES_MANAGERS,
        decode: decode_modules,
    },
    ModelMeta {
        django_label: "db.ModuleIssue",
        django_class: "ModuleIssue",
        rust_struct: "ModuleIssue",
        table: "module_issues",
        module: "module",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: ModuleIssue::ORDER_BY,
        columns_sql: ModuleIssue::COLUMNS,
        columns: MODULE_ISSUES_COLUMNS,
        managers: MODULE_ISSUES_MANAGERS,
        decode: decode_module_issues,
    },
    ModelMeta {
        django_label: "db.ModuleLink",
        django_class: "ModuleLink",
        rust_struct: "ModuleLink",
        table: "module_links",
        module: "module",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: ModuleLink::ORDER_BY,
        columns_sql: ModuleLink::COLUMNS,
        columns: MODULE_LINKS_COLUMNS,
        managers: MODULE_LINKS_MANAGERS,
        decode: decode_module_links,
    },
    ModelMeta {
        django_label: "db.ModuleMember",
        django_class: "ModuleMember",
        rust_struct: "ModuleMember",
        table: "module_members",
        module: "module",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: ModuleMember::ORDER_BY,
        columns_sql: ModuleMember::COLUMNS,
        columns: MODULE_MEMBERS_COLUMNS,
        managers: MODULE_MEMBERS_MANAGERS,
        decode: decode_module_members,
    },
    ModelMeta {
        django_label: "db.ModuleUserProperties",
        django_class: "ModuleUserProperties",
        rust_struct: "ModuleUserProperties",
        table: "module_user_properties",
        module: "module",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: ModuleUserProperties::ORDER_BY,
        columns_sql: ModuleUserProperties::COLUMNS,
        columns: MODULE_USER_PROPERTIES_COLUMNS,
        managers: MODULE_USER_PROPERTIES_MANAGERS,
        decode: decode_module_user_properties,
    },
    ModelMeta {
        django_label: "db.EmailNotificationLog",
        django_class: "EmailNotificationLog",
        rust_struct: "EmailNotificationLog",
        table: "email_notification_logs",
        module: "notification",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: EmailNotificationLog::ORDER_BY,
        columns_sql: EmailNotificationLog::COLUMNS,
        columns: EMAIL_NOTIFICATION_LOGS_COLUMNS,
        managers: EMAIL_NOTIFICATION_LOGS_MANAGERS,
        decode: decode_email_notification_logs,
    },
    ModelMeta {
        django_label: "db.Notification",
        django_class: "Notification",
        rust_struct: "Notification",
        table: "notifications",
        module: "notification",
        primary_key: "id",
        soft_delete: true,
        column_count: 21,
        ordering: Notification::ORDER_BY,
        columns_sql: Notification::COLUMNS,
        columns: NOTIFICATIONS_COLUMNS,
        managers: NOTIFICATIONS_MANAGERS,
        decode: decode_notifications,
    },
    ModelMeta {
        django_label: "db.UserNotificationPreference",
        django_class: "UserNotificationPreference",
        rust_struct: "UserNotificationPreference",
        table: "user_notification_preferences",
        module: "notification",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: UserNotificationPreference::ORDER_BY,
        columns_sql: UserNotificationPreference::COLUMNS,
        columns: USER_NOTIFICATION_PREFERENCES_COLUMNS,
        managers: USER_NOTIFICATION_PREFERENCES_MANAGERS,
        decode: decode_user_notification_preferences,
    },
    ModelMeta {
        django_label: "db.Page",
        django_class: "Page",
        rust_struct: "Page",
        table: "pages",
        module: "page",
        primary_key: "id",
        soft_delete: true,
        column_count: 26,
        ordering: Page::ORDER_BY,
        columns_sql: Page::COLUMNS,
        columns: PAGES_COLUMNS,
        managers: PAGES_MANAGERS,
        decode: decode_pages,
    },
    ModelMeta {
        django_label: "db.PageLabel",
        django_class: "PageLabel",
        rust_struct: "PageLabel",
        table: "page_labels",
        module: "page",
        primary_key: "id",
        soft_delete: true,
        column_count: 9,
        ordering: PageLabel::ORDER_BY,
        columns_sql: PageLabel::COLUMNS,
        columns: PAGE_LABELS_COLUMNS,
        managers: PAGE_LABELS_MANAGERS,
        decode: decode_page_labels,
    },
    ModelMeta {
        django_label: "db.PageLog",
        django_class: "PageLog",
        rust_struct: "PageLog",
        table: "page_logs",
        module: "page",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: PageLog::ORDER_BY,
        columns_sql: PageLog::COLUMNS,
        columns: PAGE_LOGS_COLUMNS,
        managers: PAGE_LOGS_MANAGERS,
        decode: decode_page_logs,
    },
    ModelMeta {
        django_label: "db.PageVersion",
        django_class: "PageVersion",
        rust_struct: "PageVersion",
        table: "page_versions",
        module: "page",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: PageVersion::ORDER_BY,
        columns_sql: PageVersion::COLUMNS,
        columns: PAGE_VERSIONS_COLUMNS,
        managers: PAGE_VERSIONS_MANAGERS,
        decode: decode_page_versions,
    },
    ModelMeta {
        django_label: "db.ProjectPage",
        django_class: "ProjectPage",
        rust_struct: "ProjectPage",
        table: "project_pages",
        module: "page",
        primary_key: "id",
        soft_delete: true,
        column_count: 9,
        ordering: ProjectPage::ORDER_BY,
        columns_sql: ProjectPage::COLUMNS,
        columns: PROJECT_PAGES_COLUMNS,
        managers: PROJECT_PAGES_MANAGERS,
        decode: decode_project_pages,
    },
    ModelMeta {
        django_label: "db.Project",
        django_class: "Project",
        rust_struct: "Project",
        table: "projects",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 36,
        ordering: Project::ORDER_BY,
        columns_sql: Project::COLUMNS,
        columns: PROJECTS_COLUMNS,
        managers: PROJECTS_MANAGERS,
        decode: decode_projects,
    },
    ModelMeta {
        django_label: "db.ProjectDeployBoard",
        django_class: "ProjectDeployBoard",
        rust_struct: "ProjectDeployBoard",
        table: "project_deploy_boards",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: ProjectDeployBoard::ORDER_BY,
        columns_sql: ProjectDeployBoard::COLUMNS,
        columns: PROJECT_DEPLOY_BOARDS_COLUMNS,
        managers: PROJECT_DEPLOY_BOARDS_MANAGERS,
        decode: decode_project_deploy_boards,
    },
    ModelMeta {
        django_label: "db.ProjectIdentifier",
        django_class: "ProjectIdentifier",
        rust_struct: "ProjectIdentifier",
        table: "project_identifiers",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 9,
        ordering: ProjectIdentifier::ORDER_BY,
        columns_sql: ProjectIdentifier::COLUMNS,
        columns: PROJECT_IDENTIFIERS_COLUMNS,
        managers: PROJECT_IDENTIFIERS_MANAGERS,
        decode: decode_project_identifiers,
    },
    ModelMeta {
        django_label: "db.ProjectMember",
        django_class: "ProjectMember",
        rust_struct: "ProjectMember",
        table: "project_members",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: ProjectMember::ORDER_BY,
        columns_sql: ProjectMember::COLUMNS,
        columns: PROJECT_MEMBERS_COLUMNS,
        managers: PROJECT_MEMBERS_MANAGERS,
        decode: decode_project_members,
    },
    ModelMeta {
        django_label: "db.ProjectMemberInvite",
        django_class: "ProjectMemberInvite",
        rust_struct: "ProjectMemberInvite",
        table: "project_member_invites",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: ProjectMemberInvite::ORDER_BY,
        columns_sql: ProjectMemberInvite::COLUMNS,
        columns: PROJECT_MEMBER_INVITES_COLUMNS,
        managers: PROJECT_MEMBER_INVITES_MANAGERS,
        decode: decode_project_member_invites,
    },
    ModelMeta {
        django_label: "db.ProjectPublicMember",
        django_class: "ProjectPublicMember",
        rust_struct: "ProjectPublicMember",
        table: "project_public_members",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 9,
        ordering: ProjectPublicMember::ORDER_BY,
        columns_sql: ProjectPublicMember::COLUMNS,
        columns: PROJECT_PUBLIC_MEMBERS_COLUMNS,
        managers: PROJECT_PUBLIC_MEMBERS_MANAGERS,
        decode: decode_project_public_members,
    },
    ModelMeta {
        django_label: "db.ProjectUserProperty",
        django_class: "ProjectUserProperty",
        rust_struct: "ProjectUserProperty",
        table: "project_user_properties",
        module: "project",
        primary_key: "id",
        soft_delete: true,
        column_count: 15,
        ordering: ProjectUserProperty::ORDER_BY,
        columns_sql: ProjectUserProperty::COLUMNS,
        columns: PROJECT_USER_PROPERTIES_COLUMNS,
        managers: PROJECT_USER_PROPERTIES_MANAGERS,
        decode: decode_project_user_properties,
    },
    ModelMeta {
        django_label: "db.UserRecentVisit",
        django_class: "UserRecentVisit",
        rust_struct: "UserRecentVisit",
        table: "user_recent_visits",
        module: "recent_visit",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: UserRecentVisit::ORDER_BY,
        columns_sql: UserRecentVisit::COLUMNS,
        columns: USER_RECENT_VISITS_COLUMNS,
        managers: USER_RECENT_VISITS_MANAGERS,
        decode: decode_user_recent_visits,
    },
    ModelMeta {
        django_label: "db.Session",
        django_class: "Session",
        rust_struct: "Session",
        table: "sessions",
        module: "session",
        primary_key: "session_key",
        soft_delete: false,
        column_count: 5,
        ordering: Session::ORDER_BY,
        columns_sql: Session::COLUMNS,
        columns: SESSIONS_COLUMNS,
        managers: SESSIONS_MANAGERS,
        decode: decode_sessions,
    },
    ModelMeta {
        django_label: "db.SocialLoginConnection",
        django_class: "SocialLoginConnection",
        rust_struct: "SocialLoginConnection",
        table: "social_login_connections",
        module: "social_connection",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: SocialLoginConnection::ORDER_BY,
        columns_sql: SocialLoginConnection::COLUMNS,
        columns: SOCIAL_LOGIN_CONNECTIONS_COLUMNS,
        managers: SOCIAL_LOGIN_CONNECTIONS_MANAGERS,
        decode: decode_social_login_connections,
    },
    ModelMeta {
        django_label: "db.State",
        django_class: "State",
        rust_struct: "State",
        table: "states",
        module: "state",
        primary_key: "id",
        soft_delete: true,
        column_count: 18,
        ordering: State::ORDER_BY,
        columns_sql: State::COLUMNS,
        columns: STATES_COLUMNS,
        managers: STATES_MANAGERS,
        decode: decode_states,
    },
    ModelMeta {
        django_label: "db.Sticky",
        django_class: "Sticky",
        rust_struct: "Sticky",
        table: "stickies",
        module: "sticky",
        primary_key: "id",
        soft_delete: true,
        column_count: 17,
        ordering: Sticky::ORDER_BY,
        columns_sql: Sticky::COLUMNS,
        columns: STICKIES_COLUMNS,
        managers: STICKIES_MANAGERS,
        decode: decode_stickies,
    },
    ModelMeta {
        django_label: "db.Account",
        django_class: "Account",
        rust_struct: "Account",
        table: "accounts",
        module: "user",
        primary_key: "id",
        soft_delete: false,
        column_count: 13,
        ordering: Account::ORDER_BY,
        columns_sql: Account::COLUMNS,
        columns: ACCOUNTS_COLUMNS,
        managers: ACCOUNTS_MANAGERS,
        decode: decode_accounts,
    },
    ModelMeta {
        django_label: "db.Profile",
        django_class: "Profile",
        rust_struct: "Profile",
        table: "profiles",
        module: "user",
        primary_key: "id",
        soft_delete: false,
        column_count: 29,
        ordering: Profile::ORDER_BY,
        columns_sql: Profile::COLUMNS,
        columns: PROFILES_COLUMNS,
        managers: PROFILES_MANAGERS,
        decode: decode_profiles,
    },
    ModelMeta {
        django_label: "db.User",
        django_class: "User",
        rust_struct: "User",
        table: "users",
        module: "user",
        primary_key: "id",
        soft_delete: false,
        column_count: 40,
        ordering: User::ORDER_BY,
        columns_sql: User::COLUMNS,
        columns: USERS_COLUMNS,
        managers: USERS_MANAGERS,
        decode: decode_users,
    },
    ModelMeta {
        django_label: "db.User_groups",
        django_class: "User_groups",
        rust_struct: "UserGroups",
        table: "users_groups",
        module: "user",
        primary_key: "id",
        soft_delete: false,
        column_count: 3,
        ordering: UserGroups::ORDER_BY,
        columns_sql: UserGroups::COLUMNS,
        columns: USERS_GROUPS_COLUMNS,
        managers: USERS_GROUPS_MANAGERS,
        decode: decode_users_groups,
    },
    ModelMeta {
        django_label: "db.User_user_permissions",
        django_class: "User_user_permissions",
        rust_struct: "UserUserPermissions",
        table: "users_user_permissions",
        module: "user",
        primary_key: "id",
        soft_delete: false,
        column_count: 3,
        ordering: UserUserPermissions::ORDER_BY,
        columns_sql: UserUserPermissions::COLUMNS,
        columns: USERS_USER_PERMISSIONS_COLUMNS,
        managers: USERS_USER_PERMISSIONS_MANAGERS,
        decode: decode_users_user_permissions,
    },
    ModelMeta {
        django_label: "db.IssueView",
        django_class: "IssueView",
        rust_struct: "IssueView",
        table: "issue_views",
        module: "view",
        primary_key: "id",
        soft_delete: true,
        column_count: 21,
        ordering: IssueView::ORDER_BY,
        columns_sql: IssueView::COLUMNS,
        columns: ISSUE_VIEWS_COLUMNS,
        managers: ISSUE_VIEWS_MANAGERS,
        decode: decode_issue_views,
    },
    ModelMeta {
        django_label: "db.ProjectWebhook",
        django_class: "ProjectWebhook",
        rust_struct: "ProjectWebhook",
        table: "project_webhooks",
        module: "webhook",
        primary_key: "id",
        soft_delete: true,
        column_count: 9,
        ordering: ProjectWebhook::ORDER_BY,
        columns_sql: ProjectWebhook::COLUMNS,
        columns: PROJECT_WEBHOOKS_COLUMNS,
        managers: PROJECT_WEBHOOKS_MANAGERS,
        decode: decode_project_webhooks,
    },
    ModelMeta {
        django_label: "db.Webhook",
        django_class: "Webhook",
        rust_struct: "Webhook",
        table: "webhooks",
        module: "webhook",
        primary_key: "id",
        soft_delete: true,
        column_count: 17,
        ordering: Webhook::ORDER_BY,
        columns_sql: Webhook::COLUMNS,
        columns: WEBHOOKS_COLUMNS,
        managers: WEBHOOKS_MANAGERS,
        decode: decode_webhooks,
    },
    ModelMeta {
        django_label: "db.WebhookLog",
        django_class: "WebhookLog",
        rust_struct: "WebhookLog",
        table: "webhook_logs",
        module: "webhook",
        primary_key: "id",
        soft_delete: true,
        column_count: 16,
        ordering: WebhookLog::ORDER_BY,
        columns_sql: WebhookLog::COLUMNS,
        columns: WEBHOOK_LOGS_COLUMNS,
        managers: WEBHOOK_LOGS_MANAGERS,
        decode: decode_webhook_logs,
    },
    ModelMeta {
        django_label: "db.Team",
        django_class: "Team",
        rust_struct: "Team",
        table: "teams",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: Team::ORDER_BY,
        columns_sql: Team::COLUMNS,
        columns: TEAMS_COLUMNS,
        managers: TEAMS_MANAGERS,
        decode: decode_teams,
    },
    ModelMeta {
        django_label: "db.Workspace",
        django_class: "Workspace",
        rust_struct: "Workspace",
        table: "workspaces",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: Workspace::ORDER_BY,
        columns_sql: Workspace::COLUMNS,
        columns: WORKSPACES_COLUMNS,
        managers: WORKSPACES_MANAGERS,
        decode: decode_workspaces,
    },
    ModelMeta {
        django_label: "db.WorkspaceHomePreference",
        django_class: "WorkspaceHomePreference",
        rust_struct: "WorkspaceHomePreference",
        table: "workspace_home_preferences",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: WorkspaceHomePreference::ORDER_BY,
        columns_sql: WorkspaceHomePreference::COLUMNS,
        columns: WORKSPACE_HOME_PREFERENCES_COLUMNS,
        managers: WORKSPACE_HOME_PREFERENCES_MANAGERS,
        decode: decode_workspace_home_preferences,
    },
    ModelMeta {
        django_label: "db.WorkspaceMember",
        django_class: "WorkspaceMember",
        rust_struct: "WorkspaceMember",
        table: "workspace_members",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 17,
        ordering: WorkspaceMember::ORDER_BY,
        columns_sql: WorkspaceMember::COLUMNS,
        columns: WORKSPACE_MEMBERS_COLUMNS,
        managers: WORKSPACE_MEMBERS_MANAGERS,
        decode: decode_workspace_members,
    },
    ModelMeta {
        django_label: "db.WorkspaceMemberInvite",
        django_class: "WorkspaceMemberInvite",
        rust_struct: "WorkspaceMemberInvite",
        table: "workspace_member_invites",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 13,
        ordering: WorkspaceMemberInvite::ORDER_BY,
        columns_sql: WorkspaceMemberInvite::COLUMNS,
        columns: WORKSPACE_MEMBER_INVITES_COLUMNS,
        managers: WORKSPACE_MEMBER_INVITES_MANAGERS,
        decode: decode_workspace_member_invites,
    },
    ModelMeta {
        django_label: "db.WorkspaceTheme",
        django_class: "WorkspaceTheme",
        rust_struct: "WorkspaceTheme",
        table: "workspace_themes",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: WorkspaceTheme::ORDER_BY,
        columns_sql: WorkspaceTheme::COLUMNS,
        columns: WORKSPACE_THEMES_COLUMNS,
        managers: WORKSPACE_THEMES_MANAGERS,
        decode: decode_workspace_themes,
    },
    ModelMeta {
        django_label: "db.WorkspaceUserLink",
        django_class: "WorkspaceUserLink",
        rust_struct: "WorkspaceUserLink",
        table: "workspace_user_links",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: WorkspaceUserLink::ORDER_BY,
        columns_sql: WorkspaceUserLink::COLUMNS,
        columns: WORKSPACE_USER_LINKS_COLUMNS,
        managers: WORKSPACE_USER_LINKS_MANAGERS,
        decode: decode_workspace_user_links,
    },
    ModelMeta {
        django_label: "db.WorkspaceUserPreference",
        django_class: "WorkspaceUserPreference",
        rust_struct: "WorkspaceUserPreference",
        table: "workspace_user_preferences",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 11,
        ordering: WorkspaceUserPreference::ORDER_BY,
        columns_sql: WorkspaceUserPreference::COLUMNS,
        columns: WORKSPACE_USER_PREFERENCES_COLUMNS,
        managers: WORKSPACE_USER_PREFERENCES_MANAGERS,
        decode: decode_workspace_user_preferences,
    },
    ModelMeta {
        django_label: "db.WorkspaceUserProperties",
        django_class: "WorkspaceUserProperties",
        rust_struct: "WorkspaceUserProperties",
        table: "workspace_user_properties",
        module: "workspace",
        primary_key: "id",
        soft_delete: true,
        column_count: 14,
        ordering: WorkspaceUserProperties::ORDER_BY,
        columns_sql: WorkspaceUserProperties::COLUMNS,
        columns: WORKSPACE_USER_PROPERTIES_COLUMNS,
        managers: WORKSPACE_USER_PROPERTIES_MANAGERS,
        decode: decode_workspace_user_properties,
    },
    ModelMeta {
        django_label: "license.ChangeLog",
        django_class: "ChangeLog",
        rust_struct: "ChangeLog",
        table: "changelogs",
        module: "license/instance",
        primary_key: "id",
        soft_delete: true,
        column_count: 12,
        ordering: ChangeLog::ORDER_BY,
        columns_sql: ChangeLog::COLUMNS,
        columns: CHANGELOGS_COLUMNS,
        managers: CHANGELOGS_MANAGERS,
        decode: decode_changelogs,
    },
    ModelMeta {
        django_label: "license.Instance",
        django_class: "Instance",
        rust_struct: "Instance",
        table: "instances",
        module: "license/instance",
        primary_key: "id",
        soft_delete: true,
        column_count: 22,
        ordering: Instance::ORDER_BY,
        columns_sql: Instance::COLUMNS,
        columns: INSTANCES_COLUMNS,
        managers: INSTANCES_MANAGERS,
        decode: decode_instances,
    },
    ModelMeta {
        django_label: "license.InstanceAdmin",
        django_class: "InstanceAdmin",
        rust_struct: "InstanceAdmin",
        table: "instance_admins",
        module: "license/instance",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: InstanceAdmin::ORDER_BY,
        columns_sql: InstanceAdmin::COLUMNS,
        columns: INSTANCE_ADMINS_COLUMNS,
        managers: INSTANCE_ADMINS_MANAGERS,
        decode: decode_instance_admins,
    },
    ModelMeta {
        django_label: "license.InstanceConfiguration",
        django_class: "InstanceConfiguration",
        rust_struct: "InstanceConfiguration",
        table: "instance_configurations",
        module: "license/instance",
        primary_key: "id",
        soft_delete: true,
        column_count: 10,
        ordering: InstanceConfiguration::ORDER_BY,
        columns_sql: InstanceConfiguration::COLUMNS,
        columns: INSTANCE_CONFIGURATIONS_COLUMNS,
        managers: INSTANCE_CONFIGURATIONS_MANAGERS,
        decode: decode_instance_configurations,
    },
];
