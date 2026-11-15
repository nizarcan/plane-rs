// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Generated query catalog for every model row type.
//!
//! One `SELECT <columns> FROM <table> LIMIT 0` per model, declared with
//! the exact Postgres types the row struct expects. The `schema_check`
//! integration test prepares each of them against a Django-migrated
//! database, so a column that changes type or disappears fails there
//! (DECISIONS.md D1).

use crate::catalog::{ArrayElement, Column, PgType, Query};

static ANALYTIC_VIEWS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "query",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "query_dict",
        pg_type: PgType::Jsonb,
    },
];

static ANALYTIC_VIEWS_QUERY: Query = Query {
    name: "model_analytic_views",
    sql: "SELECT \"analytic_views\".\"created_at\", \"analytic_views\".\"updated_at\", \"analytic_views\".\"created_by_id\", \"analytic_views\".\"updated_by_id\", \"analytic_views\".\"deleted_at\", \"analytic_views\".\"id\", \"analytic_views\".\"workspace_id\", \"analytic_views\".\"name\", \"analytic_views\".\"description\", \"analytic_views\".\"query\", \"analytic_views\".\"query_dict\" FROM \"analytic_views\" LIMIT 0",
    columns: ANALYTIC_VIEWS_COLUMNS,
};

static API_ACTIVITY_LOGS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "token_identifier",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "path",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "method",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "query_params",
        pg_type: PgType::Text,
    },
    Column {
        name: "headers",
        pg_type: PgType::Text,
    },
    Column {
        name: "body",
        pg_type: PgType::Text,
    },
    Column {
        name: "response_code",
        pg_type: PgType::Int4,
    },
    Column {
        name: "response_body",
        pg_type: PgType::Text,
    },
    Column {
        name: "ip_address",
        pg_type: PgType::Inet,
    },
    Column {
        name: "user_agent",
        pg_type: PgType::Varchar,
    },
];

static API_ACTIVITY_LOGS_QUERY: Query = Query {
    name: "model_api_activity_logs",
    sql: "SELECT \"api_activity_logs\".\"created_at\", \"api_activity_logs\".\"updated_at\", \"api_activity_logs\".\"created_by_id\", \"api_activity_logs\".\"updated_by_id\", \"api_activity_logs\".\"deleted_at\", \"api_activity_logs\".\"id\", \"api_activity_logs\".\"token_identifier\", \"api_activity_logs\".\"path\", \"api_activity_logs\".\"method\", \"api_activity_logs\".\"query_params\", \"api_activity_logs\".\"headers\", \"api_activity_logs\".\"body\", \"api_activity_logs\".\"response_code\", \"api_activity_logs\".\"response_body\", \"api_activity_logs\".\"ip_address\", \"api_activity_logs\".\"user_agent\" FROM \"api_activity_logs\" LIMIT 0",
    columns: API_ACTIVITY_LOGS_COLUMNS,
};

static API_TOKENS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "label",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "last_used",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_type",
        pg_type: PgType::Int2,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "expired_at",
        pg_type: PgType::Timestamptz,
    },
];

static API_TOKENS_QUERY: Query = Query {
    name: "model_api_tokens",
    sql: "SELECT \"api_tokens\".\"created_at\", \"api_tokens\".\"updated_at\", \"api_tokens\".\"created_by_id\", \"api_tokens\".\"updated_by_id\", \"api_tokens\".\"deleted_at\", \"api_tokens\".\"id\", \"api_tokens\".\"label\", \"api_tokens\".\"description\", \"api_tokens\".\"is_active\", \"api_tokens\".\"last_used\", \"api_tokens\".\"token\", \"api_tokens\".\"user_id\", \"api_tokens\".\"user_type\", \"api_tokens\".\"workspace_id\", \"api_tokens\".\"expired_at\" FROM \"api_tokens\" LIMIT 0",
    columns: API_TOKENS_COLUMNS,
};

static FILE_ASSETS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "attributes",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "asset",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "draft_issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "comment_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "page_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_deleted",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_archived",
        pg_type: PgType::Bool,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "size",
        pg_type: PgType::Float8,
    },
    Column {
        name: "is_uploaded",
        pg_type: PgType::Bool,
    },
    Column {
        name: "storage_metadata",
        pg_type: PgType::Jsonb,
    },
];

static FILE_ASSETS_QUERY: Query = Query {
    name: "model_file_assets",
    sql: "SELECT \"file_assets\".\"created_at\", \"file_assets\".\"updated_at\", \"file_assets\".\"created_by_id\", \"file_assets\".\"updated_by_id\", \"file_assets\".\"deleted_at\", \"file_assets\".\"id\", \"file_assets\".\"attributes\", \"file_assets\".\"asset\", \"file_assets\".\"user_id\", \"file_assets\".\"workspace_id\", \"file_assets\".\"draft_issue_id\", \"file_assets\".\"project_id\", \"file_assets\".\"issue_id\", \"file_assets\".\"comment_id\", \"file_assets\".\"page_id\", \"file_assets\".\"entity_type\", \"file_assets\".\"entity_identifier\", \"file_assets\".\"is_deleted\", \"file_assets\".\"is_archived\", \"file_assets\".\"external_id\", \"file_assets\".\"external_source\", \"file_assets\".\"size\", \"file_assets\".\"is_uploaded\", \"file_assets\".\"storage_metadata\" FROM \"file_assets\" LIMIT 0",
    columns: FILE_ASSETS_COLUMNS,
};

static CYCLES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "end_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "progress_snapshot",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "version",
        pg_type: PgType::Int4,
    },
];

static CYCLES_QUERY: Query = Query {
    name: "model_cycles",
    sql: "SELECT \"cycles\".\"created_at\", \"cycles\".\"updated_at\", \"cycles\".\"created_by_id\", \"cycles\".\"updated_by_id\", \"cycles\".\"deleted_at\", \"cycles\".\"id\", \"cycles\".\"project_id\", \"cycles\".\"workspace_id\", \"cycles\".\"name\", \"cycles\".\"description\", \"cycles\".\"start_date\", \"cycles\".\"end_date\", \"cycles\".\"owned_by_id\", \"cycles\".\"view_props\", \"cycles\".\"sort_order\", \"cycles\".\"external_source\", \"cycles\".\"external_id\", \"cycles\".\"progress_snapshot\", \"cycles\".\"archived_at\", \"cycles\".\"logo_props\", \"cycles\".\"timezone\", \"cycles\".\"version\" FROM \"cycles\" LIMIT 0",
    columns: CYCLES_COLUMNS,
};

static CYCLE_ISSUES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "cycle_id",
        pg_type: PgType::Uuid,
    },
];

static CYCLE_ISSUES_QUERY: Query = Query {
    name: "model_cycle_issues",
    sql: "SELECT \"cycle_issues\".\"created_at\", \"cycle_issues\".\"updated_at\", \"cycle_issues\".\"created_by_id\", \"cycle_issues\".\"updated_by_id\", \"cycle_issues\".\"deleted_at\", \"cycle_issues\".\"id\", \"cycle_issues\".\"project_id\", \"cycle_issues\".\"workspace_id\", \"cycle_issues\".\"issue_id\", \"cycle_issues\".\"cycle_id\" FROM \"cycle_issues\" LIMIT 0",
    columns: CYCLE_ISSUES_COLUMNS,
};

static CYCLE_USER_PROPERTIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "cycle_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
];

static CYCLE_USER_PROPERTIES_QUERY: Query = Query {
    name: "model_cycle_user_properties",
    sql: "SELECT \"cycle_user_properties\".\"created_at\", \"cycle_user_properties\".\"updated_at\", \"cycle_user_properties\".\"created_by_id\", \"cycle_user_properties\".\"updated_by_id\", \"cycle_user_properties\".\"deleted_at\", \"cycle_user_properties\".\"id\", \"cycle_user_properties\".\"project_id\", \"cycle_user_properties\".\"workspace_id\", \"cycle_user_properties\".\"cycle_id\", \"cycle_user_properties\".\"user_id\", \"cycle_user_properties\".\"filters\", \"cycle_user_properties\".\"display_filters\", \"cycle_user_properties\".\"display_properties\", \"cycle_user_properties\".\"rich_filters\" FROM \"cycle_user_properties\" LIMIT 0",
    columns: CYCLE_USER_PROPERTIES_COLUMNS,
};

static DEPLOY_BOARDS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "anchor",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_comments_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_reactions_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "intake_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "is_votes_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "is_activity_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_disabled",
        pg_type: PgType::Bool,
    },
];

static DEPLOY_BOARDS_QUERY: Query = Query {
    name: "model_deploy_boards",
    sql: "SELECT \"deploy_boards\".\"created_at\", \"deploy_boards\".\"updated_at\", \"deploy_boards\".\"created_by_id\", \"deploy_boards\".\"updated_by_id\", \"deploy_boards\".\"deleted_at\", \"deploy_boards\".\"id\", \"deploy_boards\".\"workspace_id\", \"deploy_boards\".\"project_id\", \"deploy_boards\".\"entity_identifier\", \"deploy_boards\".\"entity_name\", \"deploy_boards\".\"anchor\", \"deploy_boards\".\"is_comments_enabled\", \"deploy_boards\".\"is_reactions_enabled\", \"deploy_boards\".\"intake_id\", \"deploy_boards\".\"is_votes_enabled\", \"deploy_boards\".\"view_props\", \"deploy_boards\".\"is_activity_enabled\", \"deploy_boards\".\"is_disabled\" FROM \"deploy_boards\" LIMIT 0",
    columns: DEPLOY_BOARDS_COLUMNS,
};

static DESCRIPTIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
];

static DESCRIPTIONS_QUERY: Query = Query {
    name: "model_descriptions",
    sql: "SELECT \"descriptions\".\"created_at\", \"descriptions\".\"updated_at\", \"descriptions\".\"created_by_id\", \"descriptions\".\"updated_by_id\", \"descriptions\".\"deleted_at\", \"descriptions\".\"id\", \"descriptions\".\"workspace_id\", \"descriptions\".\"project_id\", \"descriptions\".\"description_json\", \"descriptions\".\"description_html\", \"descriptions\".\"description_binary\", \"descriptions\".\"description_stripped\" FROM \"descriptions\" LIMIT 0",
    columns: DESCRIPTIONS_COLUMNS,
};

static DESCRIPTION_VERSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "description_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
];

static DESCRIPTION_VERSIONS_QUERY: Query = Query {
    name: "model_description_versions",
    sql: "SELECT \"description_versions\".\"created_at\", \"description_versions\".\"updated_at\", \"description_versions\".\"created_by_id\", \"description_versions\".\"updated_by_id\", \"description_versions\".\"deleted_at\", \"description_versions\".\"id\", \"description_versions\".\"workspace_id\", \"description_versions\".\"project_id\", \"description_versions\".\"description_id\", \"description_versions\".\"description_json\", \"description_versions\".\"description_html\", \"description_versions\".\"description_binary\", \"description_versions\".\"description_stripped\" FROM \"description_versions\" LIMIT 0",
    columns: DESCRIPTION_VERSIONS_COLUMNS,
};

static DEVICES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "device_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "device_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "push_token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
];

static DEVICES_QUERY: Query = Query {
    name: "model_devices",
    sql: "SELECT \"devices\".\"created_at\", \"devices\".\"updated_at\", \"devices\".\"created_by_id\", \"devices\".\"updated_by_id\", \"devices\".\"deleted_at\", \"devices\".\"id\", \"devices\".\"user_id\", \"devices\".\"device_id\", \"devices\".\"device_type\", \"devices\".\"push_token\", \"devices\".\"is_active\" FROM \"devices\" LIMIT 0",
    columns: DEVICES_COLUMNS,
};

static DEVICE_SESSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "device_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "session_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "user_agent",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "ip_address",
        pg_type: PgType::Inet,
    },
    Column {
        name: "start_time",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "end_time",
        pg_type: PgType::Timestamptz,
    },
];

static DEVICE_SESSIONS_QUERY: Query = Query {
    name: "model_device_sessions",
    sql: "SELECT \"device_sessions\".\"created_at\", \"device_sessions\".\"updated_at\", \"device_sessions\".\"created_by_id\", \"device_sessions\".\"updated_by_id\", \"device_sessions\".\"deleted_at\", \"device_sessions\".\"id\", \"device_sessions\".\"device_id\", \"device_sessions\".\"session_id\", \"device_sessions\".\"is_active\", \"device_sessions\".\"user_agent\", \"device_sessions\".\"ip_address\", \"device_sessions\".\"start_time\", \"device_sessions\".\"end_time\" FROM \"device_sessions\" LIMIT 0",
    columns: DEVICE_SESSIONS_COLUMNS,
};

static DRAFT_ISSUES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "state_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "estimate_point_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "priority",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "completed_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "type_id",
        pg_type: PgType::Uuid,
    },
];

static DRAFT_ISSUES_QUERY: Query = Query {
    name: "model_draft_issues",
    sql: "SELECT \"draft_issues\".\"created_at\", \"draft_issues\".\"updated_at\", \"draft_issues\".\"created_by_id\", \"draft_issues\".\"updated_by_id\", \"draft_issues\".\"deleted_at\", \"draft_issues\".\"id\", \"draft_issues\".\"workspace_id\", \"draft_issues\".\"project_id\", \"draft_issues\".\"parent_id\", \"draft_issues\".\"state_id\", \"draft_issues\".\"estimate_point_id\", \"draft_issues\".\"name\", \"draft_issues\".\"description_json\", \"draft_issues\".\"description_html\", \"draft_issues\".\"description_stripped\", \"draft_issues\".\"description_binary\", \"draft_issues\".\"priority\", \"draft_issues\".\"start_date\", \"draft_issues\".\"target_date\", \"draft_issues\".\"sort_order\", \"draft_issues\".\"completed_at\", \"draft_issues\".\"external_source\", \"draft_issues\".\"external_id\", \"draft_issues\".\"type_id\" FROM \"draft_issues\" LIMIT 0",
    columns: DRAFT_ISSUES_COLUMNS,
};

static DRAFT_ISSUE_ASSIGNEES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "draft_issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "assignee_id",
        pg_type: PgType::Uuid,
    },
];

static DRAFT_ISSUE_ASSIGNEES_QUERY: Query = Query {
    name: "model_draft_issue_assignees",
    sql: "SELECT \"draft_issue_assignees\".\"created_at\", \"draft_issue_assignees\".\"updated_at\", \"draft_issue_assignees\".\"created_by_id\", \"draft_issue_assignees\".\"updated_by_id\", \"draft_issue_assignees\".\"deleted_at\", \"draft_issue_assignees\".\"id\", \"draft_issue_assignees\".\"workspace_id\", \"draft_issue_assignees\".\"project_id\", \"draft_issue_assignees\".\"draft_issue_id\", \"draft_issue_assignees\".\"assignee_id\" FROM \"draft_issue_assignees\" LIMIT 0",
    columns: DRAFT_ISSUE_ASSIGNEES_COLUMNS,
};

static DRAFT_ISSUE_CYCLES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "draft_issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "cycle_id",
        pg_type: PgType::Uuid,
    },
];

static DRAFT_ISSUE_CYCLES_QUERY: Query = Query {
    name: "model_draft_issue_cycles",
    sql: "SELECT \"draft_issue_cycles\".\"created_at\", \"draft_issue_cycles\".\"updated_at\", \"draft_issue_cycles\".\"created_by_id\", \"draft_issue_cycles\".\"updated_by_id\", \"draft_issue_cycles\".\"deleted_at\", \"draft_issue_cycles\".\"id\", \"draft_issue_cycles\".\"workspace_id\", \"draft_issue_cycles\".\"project_id\", \"draft_issue_cycles\".\"draft_issue_id\", \"draft_issue_cycles\".\"cycle_id\" FROM \"draft_issue_cycles\" LIMIT 0",
    columns: DRAFT_ISSUE_CYCLES_COLUMNS,
};

static DRAFT_ISSUE_LABELS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "draft_issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "label_id",
        pg_type: PgType::Uuid,
    },
];

static DRAFT_ISSUE_LABELS_QUERY: Query = Query {
    name: "model_draft_issue_labels",
    sql: "SELECT \"draft_issue_labels\".\"created_at\", \"draft_issue_labels\".\"updated_at\", \"draft_issue_labels\".\"created_by_id\", \"draft_issue_labels\".\"updated_by_id\", \"draft_issue_labels\".\"deleted_at\", \"draft_issue_labels\".\"id\", \"draft_issue_labels\".\"workspace_id\", \"draft_issue_labels\".\"project_id\", \"draft_issue_labels\".\"draft_issue_id\", \"draft_issue_labels\".\"label_id\" FROM \"draft_issue_labels\" LIMIT 0",
    columns: DRAFT_ISSUE_LABELS_COLUMNS,
};

static DRAFT_ISSUE_MODULES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "draft_issue_id",
        pg_type: PgType::Uuid,
    },
];

static DRAFT_ISSUE_MODULES_QUERY: Query = Query {
    name: "model_draft_issue_modules",
    sql: "SELECT \"draft_issue_modules\".\"created_at\", \"draft_issue_modules\".\"updated_at\", \"draft_issue_modules\".\"created_by_id\", \"draft_issue_modules\".\"updated_by_id\", \"draft_issue_modules\".\"deleted_at\", \"draft_issue_modules\".\"id\", \"draft_issue_modules\".\"workspace_id\", \"draft_issue_modules\".\"project_id\", \"draft_issue_modules\".\"module_id\", \"draft_issue_modules\".\"draft_issue_id\" FROM \"draft_issue_modules\" LIMIT 0",
    columns: DRAFT_ISSUE_MODULES_COLUMNS,
};

static ESTIMATES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_used",
        pg_type: PgType::Bool,
    },
];

static ESTIMATES_QUERY: Query = Query {
    name: "model_estimates",
    sql: "SELECT \"estimates\".\"created_at\", \"estimates\".\"updated_at\", \"estimates\".\"created_by_id\", \"estimates\".\"updated_by_id\", \"estimates\".\"deleted_at\", \"estimates\".\"id\", \"estimates\".\"project_id\", \"estimates\".\"workspace_id\", \"estimates\".\"name\", \"estimates\".\"description\", \"estimates\".\"type\", \"estimates\".\"last_used\" FROM \"estimates\" LIMIT 0",
    columns: ESTIMATES_COLUMNS,
};

static ESTIMATE_POINTS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "estimate_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "key",
        pg_type: PgType::Int4,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "value",
        pg_type: PgType::Varchar,
    },
];

static ESTIMATE_POINTS_QUERY: Query = Query {
    name: "model_estimate_points",
    sql: "SELECT \"estimate_points\".\"created_at\", \"estimate_points\".\"updated_at\", \"estimate_points\".\"created_by_id\", \"estimate_points\".\"updated_by_id\", \"estimate_points\".\"deleted_at\", \"estimate_points\".\"id\", \"estimate_points\".\"project_id\", \"estimate_points\".\"workspace_id\", \"estimate_points\".\"estimate_id\", \"estimate_points\".\"key\", \"estimate_points\".\"description\", \"estimate_points\".\"value\" FROM \"estimate_points\" LIMIT 0",
    columns: ESTIMATE_POINTS_COLUMNS,
};

static EXPORTERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
    },
    Column {
        name: "provider",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "status",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "reason",
        pg_type: PgType::Text,
    },
    Column {
        name: "key",
        pg_type: PgType::Text,
    },
    Column {
        name: "url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "initiated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
];

static EXPORTERS_QUERY: Query = Query {
    name: "model_exporters",
    sql: "SELECT \"exporters\".\"created_at\", \"exporters\".\"updated_at\", \"exporters\".\"created_by_id\", \"exporters\".\"updated_by_id\", \"exporters\".\"deleted_at\", \"exporters\".\"id\", \"exporters\".\"name\", \"exporters\".\"type\", \"exporters\".\"workspace_id\", \"exporters\".\"project\", \"exporters\".\"provider\", \"exporters\".\"status\", \"exporters\".\"reason\", \"exporters\".\"key\", \"exporters\".\"url\", \"exporters\".\"token\", \"exporters\".\"initiated_by_id\", \"exporters\".\"filters\", \"exporters\".\"rich_filters\" FROM \"exporters\" LIMIT 0",
    columns: EXPORTERS_COLUMNS,
};

static USER_FAVORITES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_folder",
        pg_type: PgType::Bool,
    },
    Column {
        name: "sequence",
        pg_type: PgType::Float8,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
];

static USER_FAVORITES_QUERY: Query = Query {
    name: "model_user_favorites",
    sql: "SELECT \"user_favorites\".\"created_at\", \"user_favorites\".\"updated_at\", \"user_favorites\".\"created_by_id\", \"user_favorites\".\"updated_by_id\", \"user_favorites\".\"deleted_at\", \"user_favorites\".\"id\", \"user_favorites\".\"workspace_id\", \"user_favorites\".\"project_id\", \"user_favorites\".\"user_id\", \"user_favorites\".\"entity_type\", \"user_favorites\".\"entity_identifier\", \"user_favorites\".\"name\", \"user_favorites\".\"is_folder\", \"user_favorites\".\"sequence\", \"user_favorites\".\"parent_id\" FROM \"user_favorites\" LIMIT 0",
    columns: USER_FAVORITES_COLUMNS,
};

static IMPORTERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "service",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "status",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "initiated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "config",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "data",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "token_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "imported_data",
        pg_type: PgType::Jsonb,
    },
];

static IMPORTERS_QUERY: Query = Query {
    name: "model_importers",
    sql: "SELECT \"importers\".\"created_at\", \"importers\".\"updated_at\", \"importers\".\"created_by_id\", \"importers\".\"updated_by_id\", \"importers\".\"deleted_at\", \"importers\".\"id\", \"importers\".\"project_id\", \"importers\".\"workspace_id\", \"importers\".\"service\", \"importers\".\"status\", \"importers\".\"initiated_by_id\", \"importers\".\"metadata\", \"importers\".\"config\", \"importers\".\"data\", \"importers\".\"token_id\", \"importers\".\"imported_data\" FROM \"importers\" LIMIT 0",
    columns: IMPORTERS_COLUMNS,
};

static INTAKES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "is_default",
        pg_type: PgType::Bool,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
];

static INTAKES_QUERY: Query = Query {
    name: "model_intakes",
    sql: "SELECT \"intakes\".\"created_at\", \"intakes\".\"updated_at\", \"intakes\".\"created_by_id\", \"intakes\".\"updated_by_id\", \"intakes\".\"deleted_at\", \"intakes\".\"id\", \"intakes\".\"project_id\", \"intakes\".\"workspace_id\", \"intakes\".\"name\", \"intakes\".\"description\", \"intakes\".\"is_default\", \"intakes\".\"view_props\", \"intakes\".\"logo_props\" FROM \"intakes\" LIMIT 0",
    columns: INTAKES_COLUMNS,
};

static INTAKE_ISSUES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "intake_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "status",
        pg_type: PgType::Int4,
    },
    Column {
        name: "snoozed_till",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "duplicate_to_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "source_email",
        pg_type: PgType::Text,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "extra",
        pg_type: PgType::Jsonb,
    },
];

static INTAKE_ISSUES_QUERY: Query = Query {
    name: "model_intake_issues",
    sql: "SELECT \"intake_issues\".\"created_at\", \"intake_issues\".\"updated_at\", \"intake_issues\".\"created_by_id\", \"intake_issues\".\"updated_by_id\", \"intake_issues\".\"deleted_at\", \"intake_issues\".\"id\", \"intake_issues\".\"project_id\", \"intake_issues\".\"workspace_id\", \"intake_issues\".\"intake_id\", \"intake_issues\".\"issue_id\", \"intake_issues\".\"status\", \"intake_issues\".\"snoozed_till\", \"intake_issues\".\"duplicate_to_id\", \"intake_issues\".\"source\", \"intake_issues\".\"source_email\", \"intake_issues\".\"external_source\", \"intake_issues\".\"external_id\", \"intake_issues\".\"extra\" FROM \"intake_issues\" LIMIT 0",
    columns: INTAKE_ISSUES_COLUMNS,
};

static INTEGRATIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "title",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "provider",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "network",
        pg_type: PgType::Int4,
    },
    Column {
        name: "description",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "author",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "webhook_url",
        pg_type: PgType::Text,
    },
    Column {
        name: "webhook_secret",
        pg_type: PgType::Text,
    },
    Column {
        name: "redirect_url",
        pg_type: PgType::Text,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "verified",
        pg_type: PgType::Bool,
    },
    Column {
        name: "avatar_url",
        pg_type: PgType::Text,
    },
];

static INTEGRATIONS_QUERY: Query = Query {
    name: "model_integrations",
    sql: "SELECT \"integrations\".\"created_at\", \"integrations\".\"updated_at\", \"integrations\".\"created_by_id\", \"integrations\".\"updated_by_id\", \"integrations\".\"deleted_at\", \"integrations\".\"id\", \"integrations\".\"title\", \"integrations\".\"provider\", \"integrations\".\"network\", \"integrations\".\"description\", \"integrations\".\"author\", \"integrations\".\"webhook_url\", \"integrations\".\"webhook_secret\", \"integrations\".\"redirect_url\", \"integrations\".\"metadata\", \"integrations\".\"verified\", \"integrations\".\"avatar_url\" FROM \"integrations\" LIMIT 0",
    columns: INTEGRATIONS_COLUMNS,
};

static WORKSPACE_INTEGRATIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "integration_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "api_token_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "config",
        pg_type: PgType::Jsonb,
    },
];

static WORKSPACE_INTEGRATIONS_QUERY: Query = Query {
    name: "model_workspace_integrations",
    sql: "SELECT \"workspace_integrations\".\"created_at\", \"workspace_integrations\".\"updated_at\", \"workspace_integrations\".\"created_by_id\", \"workspace_integrations\".\"updated_by_id\", \"workspace_integrations\".\"deleted_at\", \"workspace_integrations\".\"id\", \"workspace_integrations\".\"workspace_id\", \"workspace_integrations\".\"actor_id\", \"workspace_integrations\".\"integration_id\", \"workspace_integrations\".\"api_token_id\", \"workspace_integrations\".\"metadata\", \"workspace_integrations\".\"config\" FROM \"workspace_integrations\" LIMIT 0",
    columns: WORKSPACE_INTEGRATIONS_COLUMNS,
};

static GITHUB_COMMENT_SYNCS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "repo_comment_id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "comment_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_sync_id",
        pg_type: PgType::Uuid,
    },
];

static GITHUB_COMMENT_SYNCS_QUERY: Query = Query {
    name: "model_github_comment_syncs",
    sql: "SELECT \"github_comment_syncs\".\"created_at\", \"github_comment_syncs\".\"updated_at\", \"github_comment_syncs\".\"created_by_id\", \"github_comment_syncs\".\"updated_by_id\", \"github_comment_syncs\".\"deleted_at\", \"github_comment_syncs\".\"id\", \"github_comment_syncs\".\"project_id\", \"github_comment_syncs\".\"workspace_id\", \"github_comment_syncs\".\"repo_comment_id\", \"github_comment_syncs\".\"comment_id\", \"github_comment_syncs\".\"issue_sync_id\" FROM \"github_comment_syncs\" LIMIT 0",
    columns: GITHUB_COMMENT_SYNCS_COLUMNS,
};

static GITHUB_ISSUE_SYNCS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "repo_issue_id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "github_issue_id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "issue_url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "repository_sync_id",
        pg_type: PgType::Uuid,
    },
];

static GITHUB_ISSUE_SYNCS_QUERY: Query = Query {
    name: "model_github_issue_syncs",
    sql: "SELECT \"github_issue_syncs\".\"created_at\", \"github_issue_syncs\".\"updated_at\", \"github_issue_syncs\".\"created_by_id\", \"github_issue_syncs\".\"updated_by_id\", \"github_issue_syncs\".\"deleted_at\", \"github_issue_syncs\".\"id\", \"github_issue_syncs\".\"project_id\", \"github_issue_syncs\".\"workspace_id\", \"github_issue_syncs\".\"repo_issue_id\", \"github_issue_syncs\".\"github_issue_id\", \"github_issue_syncs\".\"issue_url\", \"github_issue_syncs\".\"issue_id\", \"github_issue_syncs\".\"repository_sync_id\" FROM \"github_issue_syncs\" LIMIT 0",
    columns: GITHUB_ISSUE_SYNCS_COLUMNS,
};

static GITHUB_REPOSITORIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "config",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "repository_id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "owner",
        pg_type: PgType::Varchar,
    },
];

static GITHUB_REPOSITORIES_QUERY: Query = Query {
    name: "model_github_repositories",
    sql: "SELECT \"github_repositories\".\"created_at\", \"github_repositories\".\"updated_at\", \"github_repositories\".\"created_by_id\", \"github_repositories\".\"updated_by_id\", \"github_repositories\".\"deleted_at\", \"github_repositories\".\"id\", \"github_repositories\".\"project_id\", \"github_repositories\".\"workspace_id\", \"github_repositories\".\"name\", \"github_repositories\".\"url\", \"github_repositories\".\"config\", \"github_repositories\".\"repository_id\", \"github_repositories\".\"owner\" FROM \"github_repositories\" LIMIT 0",
    columns: GITHUB_REPOSITORIES_COLUMNS,
};

static GITHUB_REPOSITORY_SYNCS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "repository_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "credentials",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_integration_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "label_id",
        pg_type: PgType::Uuid,
    },
];

static GITHUB_REPOSITORY_SYNCS_QUERY: Query = Query {
    name: "model_github_repository_syncs",
    sql: "SELECT \"github_repository_syncs\".\"created_at\", \"github_repository_syncs\".\"updated_at\", \"github_repository_syncs\".\"created_by_id\", \"github_repository_syncs\".\"updated_by_id\", \"github_repository_syncs\".\"deleted_at\", \"github_repository_syncs\".\"id\", \"github_repository_syncs\".\"project_id\", \"github_repository_syncs\".\"workspace_id\", \"github_repository_syncs\".\"repository_id\", \"github_repository_syncs\".\"credentials\", \"github_repository_syncs\".\"actor_id\", \"github_repository_syncs\".\"workspace_integration_id\", \"github_repository_syncs\".\"label_id\" FROM \"github_repository_syncs\" LIMIT 0",
    columns: GITHUB_REPOSITORY_SYNCS_COLUMNS,
};

static SLACK_PROJECT_SYNCS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "access_token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "scopes",
        pg_type: PgType::Text,
    },
    Column {
        name: "bot_user_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "webhook_url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "data",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "team_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "team_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "workspace_integration_id",
        pg_type: PgType::Uuid,
    },
];

static SLACK_PROJECT_SYNCS_QUERY: Query = Query {
    name: "model_slack_project_syncs",
    sql: "SELECT \"slack_project_syncs\".\"created_at\", \"slack_project_syncs\".\"updated_at\", \"slack_project_syncs\".\"created_by_id\", \"slack_project_syncs\".\"updated_by_id\", \"slack_project_syncs\".\"deleted_at\", \"slack_project_syncs\".\"id\", \"slack_project_syncs\".\"project_id\", \"slack_project_syncs\".\"workspace_id\", \"slack_project_syncs\".\"access_token\", \"slack_project_syncs\".\"scopes\", \"slack_project_syncs\".\"bot_user_id\", \"slack_project_syncs\".\"webhook_url\", \"slack_project_syncs\".\"data\", \"slack_project_syncs\".\"team_id\", \"slack_project_syncs\".\"team_name\", \"slack_project_syncs\".\"workspace_integration_id\" FROM \"slack_project_syncs\" LIMIT 0",
    columns: SLACK_PROJECT_SYNCS_COLUMNS,
};

static COMMENT_REACTIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "comment_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "reaction",
        pg_type: PgType::Text,
    },
];

static COMMENT_REACTIONS_QUERY: Query = Query {
    name: "model_comment_reactions",
    sql: "SELECT \"comment_reactions\".\"created_at\", \"comment_reactions\".\"updated_at\", \"comment_reactions\".\"created_by_id\", \"comment_reactions\".\"updated_by_id\", \"comment_reactions\".\"deleted_at\", \"comment_reactions\".\"id\", \"comment_reactions\".\"project_id\", \"comment_reactions\".\"workspace_id\", \"comment_reactions\".\"actor_id\", \"comment_reactions\".\"comment_id\", \"comment_reactions\".\"reaction\" FROM \"comment_reactions\" LIMIT 0",
    columns: COMMENT_REACTIONS_COLUMNS,
};

static ISSUES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "state_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "point",
        pg_type: PgType::Int4,
    },
    Column {
        name: "estimate_point_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "priority",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "sequence_id",
        pg_type: PgType::Int4,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "completed_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Date,
    },
    Column {
        name: "is_draft",
        pg_type: PgType::Bool,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "type_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUES_QUERY: Query = Query {
    name: "model_issues",
    sql: "SELECT \"issues\".\"created_at\", \"issues\".\"updated_at\", \"issues\".\"created_by_id\", \"issues\".\"updated_by_id\", \"issues\".\"deleted_at\", \"issues\".\"id\", \"issues\".\"project_id\", \"issues\".\"workspace_id\", \"issues\".\"parent_id\", \"issues\".\"state_id\", \"issues\".\"point\", \"issues\".\"estimate_point_id\", \"issues\".\"name\", \"issues\".\"description_json\", \"issues\".\"description_html\", \"issues\".\"description_stripped\", \"issues\".\"description_binary\", \"issues\".\"priority\", \"issues\".\"start_date\", \"issues\".\"target_date\", \"issues\".\"sequence_id\", \"issues\".\"sort_order\", \"issues\".\"completed_at\", \"issues\".\"archived_at\", \"issues\".\"is_draft\", \"issues\".\"external_source\", \"issues\".\"external_id\", \"issues\".\"type_id\" FROM \"issues\" LIMIT 0",
    columns: ISSUES_COLUMNS,
};

static ISSUE_ACTIVITIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "verb",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "field",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "old_value",
        pg_type: PgType::Text,
    },
    Column {
        name: "new_value",
        pg_type: PgType::Text,
    },
    Column {
        name: "comment",
        pg_type: PgType::Text,
    },
    Column {
        name: "attachments",
        pg_type: PgType::ArrayOf(ArrayElement::Varchar),
    },
    Column {
        name: "issue_comment_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "old_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "new_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "epoch",
        pg_type: PgType::Float8,
    },
];

static ISSUE_ACTIVITIES_QUERY: Query = Query {
    name: "model_issue_activities",
    sql: "SELECT \"issue_activities\".\"created_at\", \"issue_activities\".\"updated_at\", \"issue_activities\".\"created_by_id\", \"issue_activities\".\"updated_by_id\", \"issue_activities\".\"deleted_at\", \"issue_activities\".\"id\", \"issue_activities\".\"project_id\", \"issue_activities\".\"workspace_id\", \"issue_activities\".\"issue_id\", \"issue_activities\".\"verb\", \"issue_activities\".\"field\", \"issue_activities\".\"old_value\", \"issue_activities\".\"new_value\", \"issue_activities\".\"comment\", \"issue_activities\".\"attachments\", \"issue_activities\".\"issue_comment_id\", \"issue_activities\".\"actor_id\", \"issue_activities\".\"old_identifier\", \"issue_activities\".\"new_identifier\", \"issue_activities\".\"epoch\" FROM \"issue_activities\" LIMIT 0",
    columns: ISSUE_ACTIVITIES_COLUMNS,
};

static ISSUE_ASSIGNEES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "assignee_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_ASSIGNEES_QUERY: Query = Query {
    name: "model_issue_assignees",
    sql: "SELECT \"issue_assignees\".\"created_at\", \"issue_assignees\".\"updated_at\", \"issue_assignees\".\"created_by_id\", \"issue_assignees\".\"updated_by_id\", \"issue_assignees\".\"deleted_at\", \"issue_assignees\".\"id\", \"issue_assignees\".\"project_id\", \"issue_assignees\".\"workspace_id\", \"issue_assignees\".\"issue_id\", \"issue_assignees\".\"assignee_id\" FROM \"issue_assignees\" LIMIT 0",
    columns: ISSUE_ASSIGNEES_COLUMNS,
};

static ISSUE_ATTACHMENTS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "attributes",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "asset",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
];

static ISSUE_ATTACHMENTS_QUERY: Query = Query {
    name: "model_issue_attachments",
    sql: "SELECT \"issue_attachments\".\"created_at\", \"issue_attachments\".\"updated_at\", \"issue_attachments\".\"created_by_id\", \"issue_attachments\".\"updated_by_id\", \"issue_attachments\".\"deleted_at\", \"issue_attachments\".\"id\", \"issue_attachments\".\"project_id\", \"issue_attachments\".\"workspace_id\", \"issue_attachments\".\"attributes\", \"issue_attachments\".\"asset\", \"issue_attachments\".\"issue_id\", \"issue_attachments\".\"external_source\", \"issue_attachments\".\"external_id\" FROM \"issue_attachments\" LIMIT 0",
    columns: ISSUE_ATTACHMENTS_COLUMNS,
};

static ISSUE_BLOCKERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "block_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "blocked_by_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_BLOCKERS_QUERY: Query = Query {
    name: "model_issue_blockers",
    sql: "SELECT \"issue_blockers\".\"created_at\", \"issue_blockers\".\"updated_at\", \"issue_blockers\".\"created_by_id\", \"issue_blockers\".\"updated_by_id\", \"issue_blockers\".\"deleted_at\", \"issue_blockers\".\"id\", \"issue_blockers\".\"project_id\", \"issue_blockers\".\"workspace_id\", \"issue_blockers\".\"block_id\", \"issue_blockers\".\"blocked_by_id\" FROM \"issue_blockers\" LIMIT 0",
    columns: ISSUE_BLOCKERS_COLUMNS,
};

static ISSUE_COMMENTS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "comment_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "comment_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "comment_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "attachments",
        pg_type: PgType::ArrayOf(ArrayElement::Varchar),
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "access",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "edited_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_COMMENTS_QUERY: Query = Query {
    name: "model_issue_comments",
    sql: "SELECT \"issue_comments\".\"created_at\", \"issue_comments\".\"updated_at\", \"issue_comments\".\"created_by_id\", \"issue_comments\".\"updated_by_id\", \"issue_comments\".\"deleted_at\", \"issue_comments\".\"id\", \"issue_comments\".\"project_id\", \"issue_comments\".\"workspace_id\", \"issue_comments\".\"comment_stripped\", \"issue_comments\".\"comment_json\", \"issue_comments\".\"comment_html\", \"issue_comments\".\"description_id\", \"issue_comments\".\"attachments\", \"issue_comments\".\"issue_id\", \"issue_comments\".\"actor_id\", \"issue_comments\".\"access\", \"issue_comments\".\"external_source\", \"issue_comments\".\"external_id\", \"issue_comments\".\"edited_at\", \"issue_comments\".\"parent_id\" FROM \"issue_comments\" LIMIT 0",
    columns: ISSUE_COMMENTS_COLUMNS,
};

static ISSUE_DESCRIPTION_VERSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "last_saved_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_DESCRIPTION_VERSIONS_QUERY: Query = Query {
    name: "model_issue_description_versions",
    sql: "SELECT \"issue_description_versions\".\"created_at\", \"issue_description_versions\".\"updated_at\", \"issue_description_versions\".\"created_by_id\", \"issue_description_versions\".\"updated_by_id\", \"issue_description_versions\".\"deleted_at\", \"issue_description_versions\".\"id\", \"issue_description_versions\".\"project_id\", \"issue_description_versions\".\"workspace_id\", \"issue_description_versions\".\"issue_id\", \"issue_description_versions\".\"description_binary\", \"issue_description_versions\".\"description_html\", \"issue_description_versions\".\"description_stripped\", \"issue_description_versions\".\"description_json\", \"issue_description_versions\".\"last_saved_at\", \"issue_description_versions\".\"owned_by_id\" FROM \"issue_description_versions\" LIMIT 0",
    columns: ISSUE_DESCRIPTION_VERSIONS_COLUMNS,
};

static ISSUE_LABELS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "label_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_LABELS_QUERY: Query = Query {
    name: "model_issue_labels",
    sql: "SELECT \"issue_labels\".\"created_at\", \"issue_labels\".\"updated_at\", \"issue_labels\".\"created_by_id\", \"issue_labels\".\"updated_by_id\", \"issue_labels\".\"deleted_at\", \"issue_labels\".\"id\", \"issue_labels\".\"project_id\", \"issue_labels\".\"workspace_id\", \"issue_labels\".\"issue_id\", \"issue_labels\".\"label_id\" FROM \"issue_labels\" LIMIT 0",
    columns: ISSUE_LABELS_COLUMNS,
};

static ISSUE_LINKS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "title",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "url",
        pg_type: PgType::Text,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
];

static ISSUE_LINKS_QUERY: Query = Query {
    name: "model_issue_links",
    sql: "SELECT \"issue_links\".\"created_at\", \"issue_links\".\"updated_at\", \"issue_links\".\"created_by_id\", \"issue_links\".\"updated_by_id\", \"issue_links\".\"deleted_at\", \"issue_links\".\"id\", \"issue_links\".\"project_id\", \"issue_links\".\"workspace_id\", \"issue_links\".\"title\", \"issue_links\".\"url\", \"issue_links\".\"issue_id\", \"issue_links\".\"metadata\" FROM \"issue_links\" LIMIT 0",
    columns: ISSUE_LINKS_COLUMNS,
};

static ISSUE_MENTIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "mention_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_MENTIONS_QUERY: Query = Query {
    name: "model_issue_mentions",
    sql: "SELECT \"issue_mentions\".\"created_at\", \"issue_mentions\".\"updated_at\", \"issue_mentions\".\"created_by_id\", \"issue_mentions\".\"updated_by_id\", \"issue_mentions\".\"deleted_at\", \"issue_mentions\".\"id\", \"issue_mentions\".\"project_id\", \"issue_mentions\".\"workspace_id\", \"issue_mentions\".\"issue_id\", \"issue_mentions\".\"mention_id\" FROM \"issue_mentions\" LIMIT 0",
    columns: ISSUE_MENTIONS_COLUMNS,
};

static ISSUE_REACTIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "reaction",
        pg_type: PgType::Text,
    },
];

static ISSUE_REACTIONS_QUERY: Query = Query {
    name: "model_issue_reactions",
    sql: "SELECT \"issue_reactions\".\"created_at\", \"issue_reactions\".\"updated_at\", \"issue_reactions\".\"created_by_id\", \"issue_reactions\".\"updated_by_id\", \"issue_reactions\".\"deleted_at\", \"issue_reactions\".\"id\", \"issue_reactions\".\"project_id\", \"issue_reactions\".\"workspace_id\", \"issue_reactions\".\"actor_id\", \"issue_reactions\".\"issue_id\", \"issue_reactions\".\"reaction\" FROM \"issue_reactions\" LIMIT 0",
    columns: ISSUE_REACTIONS_COLUMNS,
};

static ISSUE_RELATIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "related_issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "relation_type",
        pg_type: PgType::Varchar,
    },
];

static ISSUE_RELATIONS_QUERY: Query = Query {
    name: "model_issue_relations",
    sql: "SELECT \"issue_relations\".\"created_at\", \"issue_relations\".\"updated_at\", \"issue_relations\".\"created_by_id\", \"issue_relations\".\"updated_by_id\", \"issue_relations\".\"deleted_at\", \"issue_relations\".\"id\", \"issue_relations\".\"project_id\", \"issue_relations\".\"workspace_id\", \"issue_relations\".\"issue_id\", \"issue_relations\".\"related_issue_id\", \"issue_relations\".\"relation_type\" FROM \"issue_relations\" LIMIT 0",
    columns: ISSUE_RELATIONS_COLUMNS,
};

static ISSUE_SEQUENCES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "sequence",
        pg_type: PgType::Int8,
    },
    Column {
        name: "deleted",
        pg_type: PgType::Bool,
    },
];

static ISSUE_SEQUENCES_QUERY: Query = Query {
    name: "model_issue_sequences",
    sql: "SELECT \"issue_sequences\".\"created_at\", \"issue_sequences\".\"updated_at\", \"issue_sequences\".\"created_by_id\", \"issue_sequences\".\"updated_by_id\", \"issue_sequences\".\"deleted_at\", \"issue_sequences\".\"id\", \"issue_sequences\".\"project_id\", \"issue_sequences\".\"workspace_id\", \"issue_sequences\".\"issue_id\", \"issue_sequences\".\"sequence\", \"issue_sequences\".\"deleted\" FROM \"issue_sequences\" LIMIT 0",
    columns: ISSUE_SEQUENCES_COLUMNS,
};

static ISSUE_SUBSCRIBERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "subscriber_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_SUBSCRIBERS_QUERY: Query = Query {
    name: "model_issue_subscribers",
    sql: "SELECT \"issue_subscribers\".\"created_at\", \"issue_subscribers\".\"updated_at\", \"issue_subscribers\".\"created_by_id\", \"issue_subscribers\".\"updated_by_id\", \"issue_subscribers\".\"deleted_at\", \"issue_subscribers\".\"id\", \"issue_subscribers\".\"project_id\", \"issue_subscribers\".\"workspace_id\", \"issue_subscribers\".\"issue_id\", \"issue_subscribers\".\"subscriber_id\" FROM \"issue_subscribers\" LIMIT 0",
    columns: ISSUE_SUBSCRIBERS_COLUMNS,
};

static ISSUE_VERSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "parent",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "state",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "estimate_point",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "priority",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "assignees",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
    },
    Column {
        name: "sequence_id",
        pg_type: PgType::Int4,
    },
    Column {
        name: "labels",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "completed_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Date,
    },
    Column {
        name: "is_draft",
        pg_type: PgType::Bool,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "type",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "cycle",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "modules",
        pg_type: PgType::ArrayOf(ArrayElement::Uuid),
    },
    Column {
        name: "properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "meta",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "last_saved_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "activity_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
];

static ISSUE_VERSIONS_QUERY: Query = Query {
    name: "model_issue_versions",
    sql: "SELECT \"issue_versions\".\"created_at\", \"issue_versions\".\"updated_at\", \"issue_versions\".\"created_by_id\", \"issue_versions\".\"updated_by_id\", \"issue_versions\".\"deleted_at\", \"issue_versions\".\"id\", \"issue_versions\".\"project_id\", \"issue_versions\".\"workspace_id\", \"issue_versions\".\"parent\", \"issue_versions\".\"state\", \"issue_versions\".\"estimate_point\", \"issue_versions\".\"name\", \"issue_versions\".\"priority\", \"issue_versions\".\"start_date\", \"issue_versions\".\"target_date\", \"issue_versions\".\"assignees\", \"issue_versions\".\"sequence_id\", \"issue_versions\".\"labels\", \"issue_versions\".\"sort_order\", \"issue_versions\".\"completed_at\", \"issue_versions\".\"archived_at\", \"issue_versions\".\"is_draft\", \"issue_versions\".\"external_source\", \"issue_versions\".\"external_id\", \"issue_versions\".\"type\", \"issue_versions\".\"cycle\", \"issue_versions\".\"modules\", \"issue_versions\".\"properties\", \"issue_versions\".\"meta\", \"issue_versions\".\"last_saved_at\", \"issue_versions\".\"issue_id\", \"issue_versions\".\"activity_id\", \"issue_versions\".\"owned_by_id\" FROM \"issue_versions\" LIMIT 0",
    columns: ISSUE_VERSIONS_COLUMNS,
};

static ISSUE_VOTES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "vote",
        pg_type: PgType::Int4,
    },
];

static ISSUE_VOTES_QUERY: Query = Query {
    name: "model_issue_votes",
    sql: "SELECT \"issue_votes\".\"created_at\", \"issue_votes\".\"updated_at\", \"issue_votes\".\"created_by_id\", \"issue_votes\".\"updated_by_id\", \"issue_votes\".\"deleted_at\", \"issue_votes\".\"id\", \"issue_votes\".\"project_id\", \"issue_votes\".\"workspace_id\", \"issue_votes\".\"issue_id\", \"issue_votes\".\"actor_id\", \"issue_votes\".\"vote\" FROM \"issue_votes\" LIMIT 0",
    columns: ISSUE_VOTES_COLUMNS,
};

static ISSUE_TYPES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "is_epic",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_default",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "level",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
];

static ISSUE_TYPES_QUERY: Query = Query {
    name: "model_issue_types",
    sql: "SELECT \"issue_types\".\"created_at\", \"issue_types\".\"updated_at\", \"issue_types\".\"created_by_id\", \"issue_types\".\"updated_by_id\", \"issue_types\".\"deleted_at\", \"issue_types\".\"id\", \"issue_types\".\"workspace_id\", \"issue_types\".\"name\", \"issue_types\".\"description\", \"issue_types\".\"logo_props\", \"issue_types\".\"is_epic\", \"issue_types\".\"is_default\", \"issue_types\".\"is_active\", \"issue_types\".\"level\", \"issue_types\".\"external_source\", \"issue_types\".\"external_id\" FROM \"issue_types\" LIMIT 0",
    columns: ISSUE_TYPES_COLUMNS,
};

static PROJECT_ISSUE_TYPES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_type_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "level",
        pg_type: PgType::Int4,
    },
    Column {
        name: "is_default",
        pg_type: PgType::Bool,
    },
];

static PROJECT_ISSUE_TYPES_QUERY: Query = Query {
    name: "model_project_issue_types",
    sql: "SELECT \"project_issue_types\".\"created_at\", \"project_issue_types\".\"updated_at\", \"project_issue_types\".\"created_by_id\", \"project_issue_types\".\"updated_by_id\", \"project_issue_types\".\"deleted_at\", \"project_issue_types\".\"id\", \"project_issue_types\".\"project_id\", \"project_issue_types\".\"workspace_id\", \"project_issue_types\".\"issue_type_id\", \"project_issue_types\".\"level\", \"project_issue_types\".\"is_default\" FROM \"project_issue_types\" LIMIT 0",
    columns: PROJECT_ISSUE_TYPES_COLUMNS,
};

static LABELS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
];

static LABELS_QUERY: Query = Query {
    name: "model_labels",
    sql: "SELECT \"labels\".\"created_at\", \"labels\".\"updated_at\", \"labels\".\"created_by_id\", \"labels\".\"updated_by_id\", \"labels\".\"deleted_at\", \"labels\".\"id\", \"labels\".\"workspace_id\", \"labels\".\"project_id\", \"labels\".\"parent_id\", \"labels\".\"name\", \"labels\".\"description\", \"labels\".\"color\", \"labels\".\"sort_order\", \"labels\".\"external_source\", \"labels\".\"external_id\" FROM \"labels\" LIMIT 0",
    columns: LABELS_COLUMNS,
};

static MODULES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_text",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "start_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "target_date",
        pg_type: PgType::Date,
    },
    Column {
        name: "status",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "lead_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
];

static MODULES_QUERY: Query = Query {
    name: "model_modules",
    sql: "SELECT \"modules\".\"created_at\", \"modules\".\"updated_at\", \"modules\".\"created_by_id\", \"modules\".\"updated_by_id\", \"modules\".\"deleted_at\", \"modules\".\"id\", \"modules\".\"project_id\", \"modules\".\"workspace_id\", \"modules\".\"name\", \"modules\".\"description\", \"modules\".\"description_text\", \"modules\".\"description_html\", \"modules\".\"start_date\", \"modules\".\"target_date\", \"modules\".\"status\", \"modules\".\"lead_id\", \"modules\".\"view_props\", \"modules\".\"sort_order\", \"modules\".\"external_source\", \"modules\".\"external_id\", \"modules\".\"archived_at\", \"modules\".\"logo_props\" FROM \"modules\" LIMIT 0",
    columns: MODULES_COLUMNS,
};

static MODULE_ISSUES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "issue_id",
        pg_type: PgType::Uuid,
    },
];

static MODULE_ISSUES_QUERY: Query = Query {
    name: "model_module_issues",
    sql: "SELECT \"module_issues\".\"created_at\", \"module_issues\".\"updated_at\", \"module_issues\".\"created_by_id\", \"module_issues\".\"updated_by_id\", \"module_issues\".\"deleted_at\", \"module_issues\".\"id\", \"module_issues\".\"project_id\", \"module_issues\".\"workspace_id\", \"module_issues\".\"module_id\", \"module_issues\".\"issue_id\" FROM \"module_issues\" LIMIT 0",
    columns: MODULE_ISSUES_COLUMNS,
};

static MODULE_LINKS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "title",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
];

static MODULE_LINKS_QUERY: Query = Query {
    name: "model_module_links",
    sql: "SELECT \"module_links\".\"created_at\", \"module_links\".\"updated_at\", \"module_links\".\"created_by_id\", \"module_links\".\"updated_by_id\", \"module_links\".\"deleted_at\", \"module_links\".\"id\", \"module_links\".\"project_id\", \"module_links\".\"workspace_id\", \"module_links\".\"title\", \"module_links\".\"url\", \"module_links\".\"module_id\", \"module_links\".\"metadata\" FROM \"module_links\" LIMIT 0",
    columns: MODULE_LINKS_COLUMNS,
};

static MODULE_MEMBERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "member_id",
        pg_type: PgType::Uuid,
    },
];

static MODULE_MEMBERS_QUERY: Query = Query {
    name: "model_module_members",
    sql: "SELECT \"module_members\".\"created_at\", \"module_members\".\"updated_at\", \"module_members\".\"created_by_id\", \"module_members\".\"updated_by_id\", \"module_members\".\"deleted_at\", \"module_members\".\"id\", \"module_members\".\"project_id\", \"module_members\".\"workspace_id\", \"module_members\".\"module_id\", \"module_members\".\"member_id\" FROM \"module_members\" LIMIT 0",
    columns: MODULE_MEMBERS_COLUMNS,
};

static MODULE_USER_PROPERTIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "module_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
];

static MODULE_USER_PROPERTIES_QUERY: Query = Query {
    name: "model_module_user_properties",
    sql: "SELECT \"module_user_properties\".\"created_at\", \"module_user_properties\".\"updated_at\", \"module_user_properties\".\"created_by_id\", \"module_user_properties\".\"updated_by_id\", \"module_user_properties\".\"deleted_at\", \"module_user_properties\".\"id\", \"module_user_properties\".\"project_id\", \"module_user_properties\".\"workspace_id\", \"module_user_properties\".\"module_id\", \"module_user_properties\".\"user_id\", \"module_user_properties\".\"filters\", \"module_user_properties\".\"display_filters\", \"module_user_properties\".\"display_properties\", \"module_user_properties\".\"rich_filters\" FROM \"module_user_properties\" LIMIT 0",
    columns: MODULE_USER_PROPERTIES_COLUMNS,
};

static EMAIL_NOTIFICATION_LOGS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "receiver_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "triggered_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "data",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "processed_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "sent_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "entity",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "old_value",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "new_value",
        pg_type: PgType::Varchar,
    },
];

static EMAIL_NOTIFICATION_LOGS_QUERY: Query = Query {
    name: "model_email_notification_logs",
    sql: "SELECT \"email_notification_logs\".\"created_at\", \"email_notification_logs\".\"updated_at\", \"email_notification_logs\".\"created_by_id\", \"email_notification_logs\".\"updated_by_id\", \"email_notification_logs\".\"deleted_at\", \"email_notification_logs\".\"id\", \"email_notification_logs\".\"receiver_id\", \"email_notification_logs\".\"triggered_by_id\", \"email_notification_logs\".\"entity_identifier\", \"email_notification_logs\".\"entity_name\", \"email_notification_logs\".\"data\", \"email_notification_logs\".\"processed_at\", \"email_notification_logs\".\"sent_at\", \"email_notification_logs\".\"entity\", \"email_notification_logs\".\"old_value\", \"email_notification_logs\".\"new_value\" FROM \"email_notification_logs\" LIMIT 0",
    columns: EMAIL_NOTIFICATION_LOGS_COLUMNS,
};

static NOTIFICATIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "data",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "title",
        pg_type: PgType::Text,
    },
    Column {
        name: "message",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "message_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "message_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "sender",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "triggered_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "receiver_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "read_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "snoozed_till",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
];

static NOTIFICATIONS_QUERY: Query = Query {
    name: "model_notifications",
    sql: "SELECT \"notifications\".\"created_at\", \"notifications\".\"updated_at\", \"notifications\".\"created_by_id\", \"notifications\".\"updated_by_id\", \"notifications\".\"deleted_at\", \"notifications\".\"id\", \"notifications\".\"workspace_id\", \"notifications\".\"project_id\", \"notifications\".\"data\", \"notifications\".\"entity_identifier\", \"notifications\".\"entity_name\", \"notifications\".\"title\", \"notifications\".\"message\", \"notifications\".\"message_html\", \"notifications\".\"message_stripped\", \"notifications\".\"sender\", \"notifications\".\"triggered_by_id\", \"notifications\".\"receiver_id\", \"notifications\".\"read_at\", \"notifications\".\"snoozed_till\", \"notifications\".\"archived_at\" FROM \"notifications\" LIMIT 0",
    columns: NOTIFICATIONS_COLUMNS,
};

static USER_NOTIFICATION_PREFERENCES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "property_change",
        pg_type: PgType::Bool,
    },
    Column {
        name: "state_change",
        pg_type: PgType::Bool,
    },
    Column {
        name: "comment",
        pg_type: PgType::Bool,
    },
    Column {
        name: "mention",
        pg_type: PgType::Bool,
    },
    Column {
        name: "issue_completed",
        pg_type: PgType::Bool,
    },
];

static USER_NOTIFICATION_PREFERENCES_QUERY: Query = Query {
    name: "model_user_notification_preferences",
    sql: "SELECT \"user_notification_preferences\".\"created_at\", \"user_notification_preferences\".\"updated_at\", \"user_notification_preferences\".\"created_by_id\", \"user_notification_preferences\".\"updated_by_id\", \"user_notification_preferences\".\"deleted_at\", \"user_notification_preferences\".\"id\", \"user_notification_preferences\".\"user_id\", \"user_notification_preferences\".\"workspace_id\", \"user_notification_preferences\".\"project_id\", \"user_notification_preferences\".\"property_change\", \"user_notification_preferences\".\"state_change\", \"user_notification_preferences\".\"comment\", \"user_notification_preferences\".\"mention\", \"user_notification_preferences\".\"issue_completed\" FROM \"user_notification_preferences\" LIMIT 0",
    columns: USER_NOTIFICATION_PREFERENCES_COLUMNS,
};

static PAGES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "access",
        pg_type: PgType::Int2,
    },
    Column {
        name: "color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "parent_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Date,
    },
    Column {
        name: "is_locked",
        pg_type: PgType::Bool,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "is_global",
        pg_type: PgType::Bool,
    },
    Column {
        name: "moved_to_page",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "moved_to_project",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
];

static PAGES_QUERY: Query = Query {
    name: "model_pages",
    sql: "SELECT \"pages\".\"created_at\", \"pages\".\"updated_at\", \"pages\".\"created_by_id\", \"pages\".\"updated_by_id\", \"pages\".\"deleted_at\", \"pages\".\"id\", \"pages\".\"workspace_id\", \"pages\".\"name\", \"pages\".\"description_json\", \"pages\".\"description_binary\", \"pages\".\"description_html\", \"pages\".\"description_stripped\", \"pages\".\"owned_by_id\", \"pages\".\"access\", \"pages\".\"color\", \"pages\".\"parent_id\", \"pages\".\"archived_at\", \"pages\".\"is_locked\", \"pages\".\"view_props\", \"pages\".\"logo_props\", \"pages\".\"is_global\", \"pages\".\"moved_to_page\", \"pages\".\"moved_to_project\", \"pages\".\"sort_order\", \"pages\".\"external_id\", \"pages\".\"external_source\" FROM \"pages\" LIMIT 0",
    columns: PAGES_COLUMNS,
};

static PAGE_LABELS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "label_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "page_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
];

static PAGE_LABELS_QUERY: Query = Query {
    name: "model_page_labels",
    sql: "SELECT \"page_labels\".\"created_at\", \"page_labels\".\"updated_at\", \"page_labels\".\"created_by_id\", \"page_labels\".\"updated_by_id\", \"page_labels\".\"deleted_at\", \"page_labels\".\"id\", \"page_labels\".\"label_id\", \"page_labels\".\"page_id\", \"page_labels\".\"workspace_id\" FROM \"page_labels\" LIMIT 0",
    columns: PAGE_LABELS_COLUMNS,
};

static PAGE_LOGS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "transaction",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "page_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "entity_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
];

static PAGE_LOGS_QUERY: Query = Query {
    name: "model_page_logs",
    sql: "SELECT \"page_logs\".\"created_at\", \"page_logs\".\"updated_at\", \"page_logs\".\"created_by_id\", \"page_logs\".\"updated_by_id\", \"page_logs\".\"deleted_at\", \"page_logs\".\"id\", \"page_logs\".\"transaction\", \"page_logs\".\"page_id\", \"page_logs\".\"entity_identifier\", \"page_logs\".\"entity_name\", \"page_logs\".\"entity_type\", \"page_logs\".\"workspace_id\" FROM \"page_logs\" LIMIT 0",
    columns: PAGE_LOGS_COLUMNS,
};

static PAGE_VERSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "page_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "last_saved_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_json",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sub_pages_data",
        pg_type: PgType::Jsonb,
    },
];

static PAGE_VERSIONS_QUERY: Query = Query {
    name: "model_page_versions",
    sql: "SELECT \"page_versions\".\"created_at\", \"page_versions\".\"updated_at\", \"page_versions\".\"created_by_id\", \"page_versions\".\"updated_by_id\", \"page_versions\".\"deleted_at\", \"page_versions\".\"id\", \"page_versions\".\"workspace_id\", \"page_versions\".\"page_id\", \"page_versions\".\"last_saved_at\", \"page_versions\".\"owned_by_id\", \"page_versions\".\"description_binary\", \"page_versions\".\"description_html\", \"page_versions\".\"description_stripped\", \"page_versions\".\"description_json\", \"page_versions\".\"sub_pages_data\" FROM \"page_versions\" LIMIT 0",
    columns: PAGE_VERSIONS_COLUMNS,
};

static PROJECT_PAGES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "page_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
];

static PROJECT_PAGES_QUERY: Query = Query {
    name: "model_project_pages",
    sql: "SELECT \"project_pages\".\"created_at\", \"project_pages\".\"updated_at\", \"project_pages\".\"created_by_id\", \"project_pages\".\"updated_by_id\", \"project_pages\".\"deleted_at\", \"project_pages\".\"id\", \"project_pages\".\"project_id\", \"project_pages\".\"page_id\", \"project_pages\".\"workspace_id\" FROM \"project_pages\" LIMIT 0",
    columns: PROJECT_PAGES_COLUMNS,
};

static PROJECTS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_text",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "network",
        pg_type: PgType::Int2,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "identifier",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "default_assignee_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_lead_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "emoji",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "icon_prop",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "module_view",
        pg_type: PgType::Bool,
    },
    Column {
        name: "cycle_view",
        pg_type: PgType::Bool,
    },
    Column {
        name: "issue_views_view",
        pg_type: PgType::Bool,
    },
    Column {
        name: "page_view",
        pg_type: PgType::Bool,
    },
    Column {
        name: "intake_view",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_time_tracking_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_issue_type_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "guest_view_all_features",
        pg_type: PgType::Bool,
    },
    Column {
        name: "cover_image",
        pg_type: PgType::Text,
    },
    Column {
        name: "cover_image_asset_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "estimate_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "archive_in",
        pg_type: PgType::Int4,
    },
    Column {
        name: "close_in",
        pg_type: PgType::Int4,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "default_state_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
];

static PROJECTS_QUERY: Query = Query {
    name: "model_projects",
    sql: "SELECT \"projects\".\"created_at\", \"projects\".\"updated_at\", \"projects\".\"created_by_id\", \"projects\".\"updated_by_id\", \"projects\".\"deleted_at\", \"projects\".\"id\", \"projects\".\"name\", \"projects\".\"description\", \"projects\".\"description_text\", \"projects\".\"description_html\", \"projects\".\"network\", \"projects\".\"workspace_id\", \"projects\".\"identifier\", \"projects\".\"default_assignee_id\", \"projects\".\"project_lead_id\", \"projects\".\"emoji\", \"projects\".\"icon_prop\", \"projects\".\"module_view\", \"projects\".\"cycle_view\", \"projects\".\"issue_views_view\", \"projects\".\"page_view\", \"projects\".\"intake_view\", \"projects\".\"is_time_tracking_enabled\", \"projects\".\"is_issue_type_enabled\", \"projects\".\"guest_view_all_features\", \"projects\".\"cover_image\", \"projects\".\"cover_image_asset_id\", \"projects\".\"estimate_id\", \"projects\".\"archive_in\", \"projects\".\"close_in\", \"projects\".\"logo_props\", \"projects\".\"default_state_id\", \"projects\".\"archived_at\", \"projects\".\"timezone\", \"projects\".\"external_source\", \"projects\".\"external_id\" FROM \"projects\" LIMIT 0",
    columns: PROJECTS_COLUMNS,
};

static PROJECT_DEPLOY_BOARDS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "anchor",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "comments",
        pg_type: PgType::Bool,
    },
    Column {
        name: "reactions",
        pg_type: PgType::Bool,
    },
    Column {
        name: "intake_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "votes",
        pg_type: PgType::Bool,
    },
    Column {
        name: "views",
        pg_type: PgType::Jsonb,
    },
];

static PROJECT_DEPLOY_BOARDS_QUERY: Query = Query {
    name: "model_project_deploy_boards",
    sql: "SELECT \"project_deploy_boards\".\"created_at\", \"project_deploy_boards\".\"updated_at\", \"project_deploy_boards\".\"created_by_id\", \"project_deploy_boards\".\"updated_by_id\", \"project_deploy_boards\".\"deleted_at\", \"project_deploy_boards\".\"id\", \"project_deploy_boards\".\"project_id\", \"project_deploy_boards\".\"workspace_id\", \"project_deploy_boards\".\"anchor\", \"project_deploy_boards\".\"comments\", \"project_deploy_boards\".\"reactions\", \"project_deploy_boards\".\"intake_id\", \"project_deploy_boards\".\"votes\", \"project_deploy_boards\".\"views\" FROM \"project_deploy_boards\" LIMIT 0",
    columns: PROJECT_DEPLOY_BOARDS_COLUMNS,
};

static PROJECT_IDENTIFIERS_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
];

static PROJECT_IDENTIFIERS_QUERY: Query = Query {
    name: "model_project_identifiers",
    sql: "SELECT \"project_identifiers\".\"id\", \"project_identifiers\".\"created_at\", \"project_identifiers\".\"updated_at\", \"project_identifiers\".\"created_by_id\", \"project_identifiers\".\"updated_by_id\", \"project_identifiers\".\"deleted_at\", \"project_identifiers\".\"workspace_id\", \"project_identifiers\".\"project_id\", \"project_identifiers\".\"name\" FROM \"project_identifiers\" LIMIT 0",
    columns: PROJECT_IDENTIFIERS_COLUMNS,
};

static PROJECT_MEMBERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "member_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "comment",
        pg_type: PgType::Text,
    },
    Column {
        name: "role",
        pg_type: PgType::Int2,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "default_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "preferences",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
];

static PROJECT_MEMBERS_QUERY: Query = Query {
    name: "model_project_members",
    sql: "SELECT \"project_members\".\"created_at\", \"project_members\".\"updated_at\", \"project_members\".\"created_by_id\", \"project_members\".\"updated_by_id\", \"project_members\".\"deleted_at\", \"project_members\".\"id\", \"project_members\".\"project_id\", \"project_members\".\"workspace_id\", \"project_members\".\"member_id\", \"project_members\".\"comment\", \"project_members\".\"role\", \"project_members\".\"view_props\", \"project_members\".\"default_props\", \"project_members\".\"preferences\", \"project_members\".\"sort_order\", \"project_members\".\"is_active\" FROM \"project_members\" LIMIT 0",
    columns: PROJECT_MEMBERS_COLUMNS,
};

static PROJECT_MEMBER_INVITES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "email",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "accepted",
        pg_type: PgType::Bool,
    },
    Column {
        name: "token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "message",
        pg_type: PgType::Text,
    },
    Column {
        name: "responded_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "role",
        pg_type: PgType::Int2,
    },
];

static PROJECT_MEMBER_INVITES_QUERY: Query = Query {
    name: "model_project_member_invites",
    sql: "SELECT \"project_member_invites\".\"created_at\", \"project_member_invites\".\"updated_at\", \"project_member_invites\".\"created_by_id\", \"project_member_invites\".\"updated_by_id\", \"project_member_invites\".\"deleted_at\", \"project_member_invites\".\"id\", \"project_member_invites\".\"project_id\", \"project_member_invites\".\"workspace_id\", \"project_member_invites\".\"email\", \"project_member_invites\".\"accepted\", \"project_member_invites\".\"token\", \"project_member_invites\".\"message\", \"project_member_invites\".\"responded_at\", \"project_member_invites\".\"role\" FROM \"project_member_invites\" LIMIT 0",
    columns: PROJECT_MEMBER_INVITES_COLUMNS,
};

static PROJECT_PUBLIC_MEMBERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "member_id",
        pg_type: PgType::Uuid,
    },
];

static PROJECT_PUBLIC_MEMBERS_QUERY: Query = Query {
    name: "model_project_public_members",
    sql: "SELECT \"project_public_members\".\"created_at\", \"project_public_members\".\"updated_at\", \"project_public_members\".\"created_by_id\", \"project_public_members\".\"updated_by_id\", \"project_public_members\".\"deleted_at\", \"project_public_members\".\"id\", \"project_public_members\".\"project_id\", \"project_public_members\".\"workspace_id\", \"project_public_members\".\"member_id\" FROM \"project_public_members\" LIMIT 0",
    columns: PROJECT_PUBLIC_MEMBERS_COLUMNS,
};

static PROJECT_USER_PROPERTIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "preferences",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
];

static PROJECT_USER_PROPERTIES_QUERY: Query = Query {
    name: "model_project_user_properties",
    sql: "SELECT \"project_user_properties\".\"created_at\", \"project_user_properties\".\"updated_at\", \"project_user_properties\".\"created_by_id\", \"project_user_properties\".\"updated_by_id\", \"project_user_properties\".\"deleted_at\", \"project_user_properties\".\"id\", \"project_user_properties\".\"project_id\", \"project_user_properties\".\"workspace_id\", \"project_user_properties\".\"user_id\", \"project_user_properties\".\"filters\", \"project_user_properties\".\"display_filters\", \"project_user_properties\".\"display_properties\", \"project_user_properties\".\"rich_filters\", \"project_user_properties\".\"preferences\", \"project_user_properties\".\"sort_order\" FROM \"project_user_properties\" LIMIT 0",
    columns: PROJECT_USER_PROPERTIES_COLUMNS,
};

static USER_RECENT_VISITS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_identifier",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "entity_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "visited_at",
        pg_type: PgType::Timestamptz,
    },
];

static USER_RECENT_VISITS_QUERY: Query = Query {
    name: "model_user_recent_visits",
    sql: "SELECT \"user_recent_visits\".\"created_at\", \"user_recent_visits\".\"updated_at\", \"user_recent_visits\".\"created_by_id\", \"user_recent_visits\".\"updated_by_id\", \"user_recent_visits\".\"deleted_at\", \"user_recent_visits\".\"id\", \"user_recent_visits\".\"workspace_id\", \"user_recent_visits\".\"project_id\", \"user_recent_visits\".\"entity_identifier\", \"user_recent_visits\".\"entity_name\", \"user_recent_visits\".\"user_id\", \"user_recent_visits\".\"visited_at\" FROM \"user_recent_visits\" LIMIT 0",
    columns: USER_RECENT_VISITS_COLUMNS,
};

static SESSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "session_data",
        pg_type: PgType::Text,
    },
    Column {
        name: "expire_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "device_info",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "session_key",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Varchar,
    },
];

static SESSIONS_QUERY: Query = Query {
    name: "model_sessions",
    sql: "SELECT \"sessions\".\"session_data\", \"sessions\".\"expire_date\", \"sessions\".\"device_info\", \"sessions\".\"session_key\", \"sessions\".\"user_id\" FROM \"sessions\" LIMIT 0",
    columns: SESSIONS_COLUMNS,
};

static SOCIAL_LOGIN_CONNECTIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "medium",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_login_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_received_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "token_data",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "extra_data",
        pg_type: PgType::Jsonb,
    },
];

static SOCIAL_LOGIN_CONNECTIONS_QUERY: Query = Query {
    name: "model_social_login_connections",
    sql: "SELECT \"social_login_connections\".\"created_at\", \"social_login_connections\".\"updated_at\", \"social_login_connections\".\"created_by_id\", \"social_login_connections\".\"updated_by_id\", \"social_login_connections\".\"deleted_at\", \"social_login_connections\".\"id\", \"social_login_connections\".\"medium\", \"social_login_connections\".\"last_login_at\", \"social_login_connections\".\"last_received_at\", \"social_login_connections\".\"user_id\", \"social_login_connections\".\"token_data\", \"social_login_connections\".\"extra_data\" FROM \"social_login_connections\" LIMIT 0",
    columns: SOCIAL_LOGIN_CONNECTIONS_COLUMNS,
};

static STATES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "slug",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "sequence",
        pg_type: PgType::Float8,
    },
    Column {
        name: "group",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_triage",
        pg_type: PgType::Bool,
    },
    Column {
        name: "default",
        pg_type: PgType::Bool,
    },
    Column {
        name: "external_source",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "external_id",
        pg_type: PgType::Varchar,
    },
];

static STATES_QUERY: Query = Query {
    name: "model_states",
    sql: "SELECT \"states\".\"created_at\", \"states\".\"updated_at\", \"states\".\"created_by_id\", \"states\".\"updated_by_id\", \"states\".\"deleted_at\", \"states\".\"id\", \"states\".\"project_id\", \"states\".\"workspace_id\", \"states\".\"name\", \"states\".\"description\", \"states\".\"color\", \"states\".\"slug\", \"states\".\"sequence\", \"states\".\"group\", \"states\".\"is_triage\", \"states\".\"default\", \"states\".\"external_source\", \"states\".\"external_id\" FROM \"states\" LIMIT 0",
    columns: STATES_COLUMNS,
};

static STICKIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Text,
    },
    Column {
        name: "description",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "description_html",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_stripped",
        pg_type: PgType::Text,
    },
    Column {
        name: "description_binary",
        pg_type: PgType::Bytea,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "background_color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "owner_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
];

static STICKIES_QUERY: Query = Query {
    name: "model_stickies",
    sql: "SELECT \"stickies\".\"created_at\", \"stickies\".\"updated_at\", \"stickies\".\"created_by_id\", \"stickies\".\"updated_by_id\", \"stickies\".\"deleted_at\", \"stickies\".\"id\", \"stickies\".\"name\", \"stickies\".\"description\", \"stickies\".\"description_html\", \"stickies\".\"description_stripped\", \"stickies\".\"description_binary\", \"stickies\".\"logo_props\", \"stickies\".\"color\", \"stickies\".\"background_color\", \"stickies\".\"workspace_id\", \"stickies\".\"owner_id\", \"stickies\".\"sort_order\" FROM \"stickies\" LIMIT 0",
    columns: STICKIES_COLUMNS,
};

static ACCOUNTS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "provider_account_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "provider",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "access_token",
        pg_type: PgType::Text,
    },
    Column {
        name: "access_token_expired_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "refresh_token",
        pg_type: PgType::Text,
    },
    Column {
        name: "refresh_token_expired_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_connected_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id_token",
        pg_type: PgType::Text,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
];

static ACCOUNTS_QUERY: Query = Query {
    name: "model_accounts",
    sql: "SELECT \"accounts\".\"created_at\", \"accounts\".\"updated_at\", \"accounts\".\"id\", \"accounts\".\"user_id\", \"accounts\".\"provider_account_id\", \"accounts\".\"provider\", \"accounts\".\"access_token\", \"accounts\".\"access_token_expired_at\", \"accounts\".\"refresh_token\", \"accounts\".\"refresh_token_expired_at\", \"accounts\".\"last_connected_at\", \"accounts\".\"id_token\", \"accounts\".\"metadata\" FROM \"accounts\" LIMIT 0",
    columns: ACCOUNTS_COLUMNS,
};

static PROFILES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "theme",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "is_app_rail_docked",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_tour_completed",
        pg_type: PgType::Bool,
    },
    Column {
        name: "onboarding_step",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "use_case",
        pg_type: PgType::Text,
    },
    Column {
        name: "role",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_onboarded",
        pg_type: PgType::Bool,
    },
    Column {
        name: "last_workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "billing_address_country",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "billing_address",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "has_billing_address",
        pg_type: PgType::Bool,
    },
    Column {
        name: "company_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "notification_view_mode",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_smooth_cursor_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_mobile_onboarded",
        pg_type: PgType::Bool,
    },
    Column {
        name: "mobile_onboarding_step",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "mobile_timezone_auto_set",
        pg_type: PgType::Bool,
    },
    Column {
        name: "language",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "start_of_the_week",
        pg_type: PgType::Int2,
    },
    Column {
        name: "goals",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "background_color",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_navigation_tour_completed",
        pg_type: PgType::Bool,
    },
    Column {
        name: "has_marketing_email_consent",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_subscribed_to_changelog",
        pg_type: PgType::Bool,
    },
    Column {
        name: "product_tour",
        pg_type: PgType::Jsonb,
    },
];

static PROFILES_QUERY: Query = Query {
    name: "model_profiles",
    sql: "SELECT \"profiles\".\"created_at\", \"profiles\".\"updated_at\", \"profiles\".\"id\", \"profiles\".\"user_id\", \"profiles\".\"theme\", \"profiles\".\"is_app_rail_docked\", \"profiles\".\"is_tour_completed\", \"profiles\".\"onboarding_step\", \"profiles\".\"use_case\", \"profiles\".\"role\", \"profiles\".\"is_onboarded\", \"profiles\".\"last_workspace_id\", \"profiles\".\"billing_address_country\", \"profiles\".\"billing_address\", \"profiles\".\"has_billing_address\", \"profiles\".\"company_name\", \"profiles\".\"notification_view_mode\", \"profiles\".\"is_smooth_cursor_enabled\", \"profiles\".\"is_mobile_onboarded\", \"profiles\".\"mobile_onboarding_step\", \"profiles\".\"mobile_timezone_auto_set\", \"profiles\".\"language\", \"profiles\".\"start_of_the_week\", \"profiles\".\"goals\", \"profiles\".\"background_color\", \"profiles\".\"is_navigation_tour_completed\", \"profiles\".\"has_marketing_email_consent\", \"profiles\".\"is_subscribed_to_changelog\", \"profiles\".\"product_tour\" FROM \"profiles\" LIMIT 0",
    columns: PROFILES_COLUMNS,
};

static USERS_COLUMNS: &[Column] = &[
    Column {
        name: "password",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_login",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "username",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "mobile_number",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "email",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "display_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "first_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "avatar",
        pg_type: PgType::Text,
    },
    Column {
        name: "avatar_asset_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "cover_image",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "cover_image_asset_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "date_joined",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_location",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "created_location",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_superuser",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_managed",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_password_expired",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_staff",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_email_verified",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_password_autoset",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_password_reset_required",
        pg_type: PgType::Bool,
    },
    Column {
        name: "token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_active",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_login_time",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_logout_time",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "last_login_ip",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_logout_ip",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_login_medium",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "last_login_uagent",
        pg_type: PgType::Text,
    },
    Column {
        name: "token_updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "is_bot",
        pg_type: PgType::Bool,
    },
    Column {
        name: "bot_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "user_timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_email_valid",
        pg_type: PgType::Bool,
    },
    Column {
        name: "masked_at",
        pg_type: PgType::Timestamptz,
    },
];

static USERS_QUERY: Query = Query {
    name: "model_users",
    sql: "SELECT \"users\".\"password\", \"users\".\"last_login\", \"users\".\"id\", \"users\".\"username\", \"users\".\"mobile_number\", \"users\".\"email\", \"users\".\"display_name\", \"users\".\"first_name\", \"users\".\"last_name\", \"users\".\"avatar\", \"users\".\"avatar_asset_id\", \"users\".\"cover_image\", \"users\".\"cover_image_asset_id\", \"users\".\"date_joined\", \"users\".\"created_at\", \"users\".\"updated_at\", \"users\".\"last_location\", \"users\".\"created_location\", \"users\".\"is_superuser\", \"users\".\"is_managed\", \"users\".\"is_password_expired\", \"users\".\"is_active\", \"users\".\"is_staff\", \"users\".\"is_email_verified\", \"users\".\"is_password_autoset\", \"users\".\"is_password_reset_required\", \"users\".\"token\", \"users\".\"last_active\", \"users\".\"last_login_time\", \"users\".\"last_logout_time\", \"users\".\"last_login_ip\", \"users\".\"last_logout_ip\", \"users\".\"last_login_medium\", \"users\".\"last_login_uagent\", \"users\".\"token_updated_at\", \"users\".\"is_bot\", \"users\".\"bot_type\", \"users\".\"user_timezone\", \"users\".\"is_email_valid\", \"users\".\"masked_at\" FROM \"users\" LIMIT 0",
    columns: USERS_COLUMNS,
};

static USERS_GROUPS_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "group_id",
        pg_type: PgType::Int4,
    },
];

static USERS_GROUPS_QUERY: Query = Query {
    name: "model_users_groups",
    sql: "SELECT \"users_groups\".\"id\", \"users_groups\".\"user_id\", \"users_groups\".\"group_id\" FROM \"users_groups\" LIMIT 0",
    columns: USERS_GROUPS_COLUMNS,
};

static USERS_USER_PERMISSIONS_COLUMNS: &[Column] = &[
    Column {
        name: "id",
        pg_type: PgType::Int8,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "permission_id",
        pg_type: PgType::Int4,
    },
];

static USERS_USER_PERMISSIONS_QUERY: Query = Query {
    name: "model_users_user_permissions",
    sql: "SELECT \"users_user_permissions\".\"id\", \"users_user_permissions\".\"user_id\", \"users_user_permissions\".\"permission_id\" FROM \"users_user_permissions\" LIMIT 0",
    columns: USERS_USER_PERMISSIONS_COLUMNS,
};

static ISSUE_VIEWS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "query",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "access",
        pg_type: PgType::Int2,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "owned_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "is_locked",
        pg_type: PgType::Bool,
    },
    Column {
        name: "archived_at",
        pg_type: PgType::Timestamptz,
    },
];

static ISSUE_VIEWS_QUERY: Query = Query {
    name: "model_issue_views",
    sql: "SELECT \"issue_views\".\"created_at\", \"issue_views\".\"updated_at\", \"issue_views\".\"created_by_id\", \"issue_views\".\"updated_by_id\", \"issue_views\".\"deleted_at\", \"issue_views\".\"id\", \"issue_views\".\"workspace_id\", \"issue_views\".\"project_id\", \"issue_views\".\"name\", \"issue_views\".\"description\", \"issue_views\".\"query\", \"issue_views\".\"filters\", \"issue_views\".\"display_filters\", \"issue_views\".\"display_properties\", \"issue_views\".\"rich_filters\", \"issue_views\".\"access\", \"issue_views\".\"sort_order\", \"issue_views\".\"logo_props\", \"issue_views\".\"owned_by_id\", \"issue_views\".\"is_locked\", \"issue_views\".\"archived_at\" FROM \"issue_views\" LIMIT 0",
    columns: ISSUE_VIEWS_COLUMNS,
};

static PROJECT_WEBHOOKS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "webhook_id",
        pg_type: PgType::Uuid,
    },
];

static PROJECT_WEBHOOKS_QUERY: Query = Query {
    name: "model_project_webhooks",
    sql: "SELECT \"project_webhooks\".\"created_at\", \"project_webhooks\".\"updated_at\", \"project_webhooks\".\"created_by_id\", \"project_webhooks\".\"updated_by_id\", \"project_webhooks\".\"deleted_at\", \"project_webhooks\".\"id\", \"project_webhooks\".\"project_id\", \"project_webhooks\".\"workspace_id\", \"project_webhooks\".\"webhook_id\" FROM \"project_webhooks\" LIMIT 0",
    columns: PROJECT_WEBHOOKS_COLUMNS,
};

static WEBHOOKS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "url",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "secret_key",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "project",
        pg_type: PgType::Bool,
    },
    Column {
        name: "issue",
        pg_type: PgType::Bool,
    },
    Column {
        name: "module",
        pg_type: PgType::Bool,
    },
    Column {
        name: "cycle",
        pg_type: PgType::Bool,
    },
    Column {
        name: "issue_comment",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_internal",
        pg_type: PgType::Bool,
    },
    Column {
        name: "version",
        pg_type: PgType::Varchar,
    },
];

static WEBHOOKS_QUERY: Query = Query {
    name: "model_webhooks",
    sql: "SELECT \"webhooks\".\"created_at\", \"webhooks\".\"updated_at\", \"webhooks\".\"created_by_id\", \"webhooks\".\"updated_by_id\", \"webhooks\".\"deleted_at\", \"webhooks\".\"id\", \"webhooks\".\"workspace_id\", \"webhooks\".\"url\", \"webhooks\".\"is_active\", \"webhooks\".\"secret_key\", \"webhooks\".\"project\", \"webhooks\".\"issue\", \"webhooks\".\"module\", \"webhooks\".\"cycle\", \"webhooks\".\"issue_comment\", \"webhooks\".\"is_internal\", \"webhooks\".\"version\" FROM \"webhooks\" LIMIT 0",
    columns: WEBHOOKS_COLUMNS,
};

static WEBHOOK_LOGS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "webhook",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "event_type",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "request_method",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "request_headers",
        pg_type: PgType::Text,
    },
    Column {
        name: "request_body",
        pg_type: PgType::Text,
    },
    Column {
        name: "response_status",
        pg_type: PgType::Text,
    },
    Column {
        name: "response_headers",
        pg_type: PgType::Text,
    },
    Column {
        name: "response_body",
        pg_type: PgType::Text,
    },
    Column {
        name: "retry_count",
        pg_type: PgType::Int2,
    },
];

static WEBHOOK_LOGS_QUERY: Query = Query {
    name: "model_webhook_logs",
    sql: "SELECT \"webhook_logs\".\"created_at\", \"webhook_logs\".\"updated_at\", \"webhook_logs\".\"created_by_id\", \"webhook_logs\".\"updated_by_id\", \"webhook_logs\".\"deleted_at\", \"webhook_logs\".\"id\", \"webhook_logs\".\"workspace_id\", \"webhook_logs\".\"webhook\", \"webhook_logs\".\"event_type\", \"webhook_logs\".\"request_method\", \"webhook_logs\".\"request_headers\", \"webhook_logs\".\"request_body\", \"webhook_logs\".\"response_status\", \"webhook_logs\".\"response_headers\", \"webhook_logs\".\"response_body\", \"webhook_logs\".\"retry_count\" FROM \"webhook_logs\" LIMIT 0",
    columns: WEBHOOK_LOGS_COLUMNS,
};

static TEAMS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "logo_props",
        pg_type: PgType::Jsonb,
    },
];

static TEAMS_QUERY: Query = Query {
    name: "model_teams",
    sql: "SELECT \"teams\".\"created_at\", \"teams\".\"updated_at\", \"teams\".\"created_by_id\", \"teams\".\"updated_by_id\", \"teams\".\"deleted_at\", \"teams\".\"id\", \"teams\".\"name\", \"teams\".\"description\", \"teams\".\"workspace_id\", \"teams\".\"logo_props\" FROM \"teams\" LIMIT 0",
    columns: TEAMS_COLUMNS,
};

static WORKSPACES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "logo",
        pg_type: PgType::Text,
    },
    Column {
        name: "logo_asset_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "owner_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "slug",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "organization_size",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "timezone",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "background_color",
        pg_type: PgType::Varchar,
    },
];

static WORKSPACES_QUERY: Query = Query {
    name: "model_workspaces",
    sql: "SELECT \"workspaces\".\"created_at\", \"workspaces\".\"updated_at\", \"workspaces\".\"created_by_id\", \"workspaces\".\"updated_by_id\", \"workspaces\".\"deleted_at\", \"workspaces\".\"id\", \"workspaces\".\"name\", \"workspaces\".\"logo\", \"workspaces\".\"logo_asset_id\", \"workspaces\".\"owner_id\", \"workspaces\".\"slug\", \"workspaces\".\"organization_size\", \"workspaces\".\"timezone\", \"workspaces\".\"background_color\" FROM \"workspaces\" LIMIT 0",
    columns: WORKSPACES_COLUMNS,
};

static WORKSPACE_HOME_PREFERENCES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "key",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "config",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
];

static WORKSPACE_HOME_PREFERENCES_QUERY: Query = Query {
    name: "model_workspace_home_preferences",
    sql: "SELECT \"workspace_home_preferences\".\"created_at\", \"workspace_home_preferences\".\"updated_at\", \"workspace_home_preferences\".\"created_by_id\", \"workspace_home_preferences\".\"updated_by_id\", \"workspace_home_preferences\".\"deleted_at\", \"workspace_home_preferences\".\"id\", \"workspace_home_preferences\".\"workspace_id\", \"workspace_home_preferences\".\"user_id\", \"workspace_home_preferences\".\"key\", \"workspace_home_preferences\".\"is_enabled\", \"workspace_home_preferences\".\"config\", \"workspace_home_preferences\".\"sort_order\" FROM \"workspace_home_preferences\" LIMIT 0",
    columns: WORKSPACE_HOME_PREFERENCES_COLUMNS,
};

static WORKSPACE_MEMBERS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "member_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "role",
        pg_type: PgType::Int2,
    },
    Column {
        name: "company_role",
        pg_type: PgType::Text,
    },
    Column {
        name: "view_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "default_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "issue_props",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "is_active",
        pg_type: PgType::Bool,
    },
    Column {
        name: "getting_started_checklist",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "tips",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "explored_features",
        pg_type: PgType::Jsonb,
    },
];

static WORKSPACE_MEMBERS_QUERY: Query = Query {
    name: "model_workspace_members",
    sql: "SELECT \"workspace_members\".\"created_at\", \"workspace_members\".\"updated_at\", \"workspace_members\".\"created_by_id\", \"workspace_members\".\"updated_by_id\", \"workspace_members\".\"deleted_at\", \"workspace_members\".\"id\", \"workspace_members\".\"workspace_id\", \"workspace_members\".\"member_id\", \"workspace_members\".\"role\", \"workspace_members\".\"company_role\", \"workspace_members\".\"view_props\", \"workspace_members\".\"default_props\", \"workspace_members\".\"issue_props\", \"workspace_members\".\"is_active\", \"workspace_members\".\"getting_started_checklist\", \"workspace_members\".\"tips\", \"workspace_members\".\"explored_features\" FROM \"workspace_members\" LIMIT 0",
    columns: WORKSPACE_MEMBERS_COLUMNS,
};

static WORKSPACE_MEMBER_INVITES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "email",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "accepted",
        pg_type: PgType::Bool,
    },
    Column {
        name: "token",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "message",
        pg_type: PgType::Text,
    },
    Column {
        name: "responded_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "role",
        pg_type: PgType::Int2,
    },
];

static WORKSPACE_MEMBER_INVITES_QUERY: Query = Query {
    name: "model_workspace_member_invites",
    sql: "SELECT \"workspace_member_invites\".\"created_at\", \"workspace_member_invites\".\"updated_at\", \"workspace_member_invites\".\"created_by_id\", \"workspace_member_invites\".\"updated_by_id\", \"workspace_member_invites\".\"deleted_at\", \"workspace_member_invites\".\"id\", \"workspace_member_invites\".\"workspace_id\", \"workspace_member_invites\".\"email\", \"workspace_member_invites\".\"accepted\", \"workspace_member_invites\".\"token\", \"workspace_member_invites\".\"message\", \"workspace_member_invites\".\"responded_at\", \"workspace_member_invites\".\"role\" FROM \"workspace_member_invites\" LIMIT 0",
    columns: WORKSPACE_MEMBER_INVITES_COLUMNS,
};

static WORKSPACE_THEMES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "actor_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "colors",
        pg_type: PgType::Jsonb,
    },
];

static WORKSPACE_THEMES_QUERY: Query = Query {
    name: "model_workspace_themes",
    sql: "SELECT \"workspace_themes\".\"created_at\", \"workspace_themes\".\"updated_at\", \"workspace_themes\".\"created_by_id\", \"workspace_themes\".\"updated_by_id\", \"workspace_themes\".\"deleted_at\", \"workspace_themes\".\"id\", \"workspace_themes\".\"workspace_id\", \"workspace_themes\".\"name\", \"workspace_themes\".\"actor_id\", \"workspace_themes\".\"colors\" FROM \"workspace_themes\" LIMIT 0",
    columns: WORKSPACE_THEMES_COLUMNS,
};

static WORKSPACE_USER_LINKS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "project_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "title",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "url",
        pg_type: PgType::Text,
    },
    Column {
        name: "metadata",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "owner_id",
        pg_type: PgType::Uuid,
    },
];

static WORKSPACE_USER_LINKS_QUERY: Query = Query {
    name: "model_workspace_user_links",
    sql: "SELECT \"workspace_user_links\".\"created_at\", \"workspace_user_links\".\"updated_at\", \"workspace_user_links\".\"created_by_id\", \"workspace_user_links\".\"updated_by_id\", \"workspace_user_links\".\"deleted_at\", \"workspace_user_links\".\"id\", \"workspace_user_links\".\"workspace_id\", \"workspace_user_links\".\"project_id\", \"workspace_user_links\".\"title\", \"workspace_user_links\".\"url\", \"workspace_user_links\".\"metadata\", \"workspace_user_links\".\"owner_id\" FROM \"workspace_user_links\" LIMIT 0",
    columns: WORKSPACE_USER_LINKS_COLUMNS,
};

static WORKSPACE_USER_PREFERENCES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "key",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_pinned",
        pg_type: PgType::Bool,
    },
    Column {
        name: "sort_order",
        pg_type: PgType::Float8,
    },
];

static WORKSPACE_USER_PREFERENCES_QUERY: Query = Query {
    name: "model_workspace_user_preferences",
    sql: "SELECT \"workspace_user_preferences\".\"created_at\", \"workspace_user_preferences\".\"updated_at\", \"workspace_user_preferences\".\"created_by_id\", \"workspace_user_preferences\".\"updated_by_id\", \"workspace_user_preferences\".\"deleted_at\", \"workspace_user_preferences\".\"id\", \"workspace_user_preferences\".\"workspace_id\", \"workspace_user_preferences\".\"user_id\", \"workspace_user_preferences\".\"key\", \"workspace_user_preferences\".\"is_pinned\", \"workspace_user_preferences\".\"sort_order\" FROM \"workspace_user_preferences\" LIMIT 0",
    columns: WORKSPACE_USER_PREFERENCES_COLUMNS,
};

static WORKSPACE_USER_PROPERTIES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "workspace_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "display_properties",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "rich_filters",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "navigation_project_limit",
        pg_type: PgType::Int4,
    },
    Column {
        name: "navigation_control_preference",
        pg_type: PgType::Varchar,
    },
];

static WORKSPACE_USER_PROPERTIES_QUERY: Query = Query {
    name: "model_workspace_user_properties",
    sql: "SELECT \"workspace_user_properties\".\"created_at\", \"workspace_user_properties\".\"updated_at\", \"workspace_user_properties\".\"created_by_id\", \"workspace_user_properties\".\"updated_by_id\", \"workspace_user_properties\".\"deleted_at\", \"workspace_user_properties\".\"id\", \"workspace_user_properties\".\"workspace_id\", \"workspace_user_properties\".\"user_id\", \"workspace_user_properties\".\"filters\", \"workspace_user_properties\".\"display_filters\", \"workspace_user_properties\".\"display_properties\", \"workspace_user_properties\".\"rich_filters\", \"workspace_user_properties\".\"navigation_project_limit\", \"workspace_user_properties\".\"navigation_control_preference\" FROM \"workspace_user_properties\" LIMIT 0",
    columns: WORKSPACE_USER_PROPERTIES_COLUMNS,
};

static CHANGELOGS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "title",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "description",
        pg_type: PgType::Text,
    },
    Column {
        name: "version",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "tags",
        pg_type: PgType::Jsonb,
    },
    Column {
        name: "release_date",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "is_release_candidate",
        pg_type: PgType::Bool,
    },
];

static CHANGELOGS_QUERY: Query = Query {
    name: "model_changelogs",
    sql: "SELECT \"changelogs\".\"created_at\", \"changelogs\".\"updated_at\", \"changelogs\".\"created_by_id\", \"changelogs\".\"updated_by_id\", \"changelogs\".\"deleted_at\", \"changelogs\".\"id\", \"changelogs\".\"title\", \"changelogs\".\"description\", \"changelogs\".\"version\", \"changelogs\".\"tags\", \"changelogs\".\"release_date\", \"changelogs\".\"is_release_candidate\" FROM \"changelogs\" LIMIT 0",
    columns: CHANGELOGS_COLUMNS,
};

static INSTANCES_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "instance_name",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "whitelist_emails",
        pg_type: PgType::Text,
    },
    Column {
        name: "instance_id",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "current_version",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "latest_version",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "edition",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "domain",
        pg_type: PgType::Text,
    },
    Column {
        name: "last_checked_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "namespace",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "is_telemetry_enabled",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_support_required",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_setup_done",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_signup_screen_visited",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_verified",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_test",
        pg_type: PgType::Bool,
    },
    Column {
        name: "is_current_version_deprecated",
        pg_type: PgType::Bool,
    },
];

static INSTANCES_QUERY: Query = Query {
    name: "model_instances",
    sql: "SELECT \"instances\".\"created_at\", \"instances\".\"updated_at\", \"instances\".\"created_by_id\", \"instances\".\"updated_by_id\", \"instances\".\"deleted_at\", \"instances\".\"id\", \"instances\".\"instance_name\", \"instances\".\"whitelist_emails\", \"instances\".\"instance_id\", \"instances\".\"current_version\", \"instances\".\"latest_version\", \"instances\".\"edition\", \"instances\".\"domain\", \"instances\".\"last_checked_at\", \"instances\".\"namespace\", \"instances\".\"is_telemetry_enabled\", \"instances\".\"is_support_required\", \"instances\".\"is_setup_done\", \"instances\".\"is_signup_screen_visited\", \"instances\".\"is_verified\", \"instances\".\"is_test\", \"instances\".\"is_current_version_deprecated\" FROM \"instances\" LIMIT 0",
    columns: INSTANCES_COLUMNS,
};

static INSTANCE_ADMINS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "user_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "instance_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "role",
        pg_type: PgType::Int4,
    },
    Column {
        name: "is_verified",
        pg_type: PgType::Bool,
    },
];

static INSTANCE_ADMINS_QUERY: Query = Query {
    name: "model_instance_admins",
    sql: "SELECT \"instance_admins\".\"created_at\", \"instance_admins\".\"updated_at\", \"instance_admins\".\"created_by_id\", \"instance_admins\".\"updated_by_id\", \"instance_admins\".\"deleted_at\", \"instance_admins\".\"id\", \"instance_admins\".\"user_id\", \"instance_admins\".\"instance_id\", \"instance_admins\".\"role\", \"instance_admins\".\"is_verified\" FROM \"instance_admins\" LIMIT 0",
    columns: INSTANCE_ADMINS_COLUMNS,
};

static INSTANCE_CONFIGURATIONS_COLUMNS: &[Column] = &[
    Column {
        name: "created_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "updated_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "created_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "updated_by_id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "deleted_at",
        pg_type: PgType::Timestamptz,
    },
    Column {
        name: "id",
        pg_type: PgType::Uuid,
    },
    Column {
        name: "key",
        pg_type: PgType::Varchar,
    },
    Column {
        name: "value",
        pg_type: PgType::Text,
    },
    Column {
        name: "category",
        pg_type: PgType::Text,
    },
    Column {
        name: "is_encrypted",
        pg_type: PgType::Bool,
    },
];

static INSTANCE_CONFIGURATIONS_QUERY: Query = Query {
    name: "model_instance_configurations",
    sql: "SELECT \"instance_configurations\".\"created_at\", \"instance_configurations\".\"updated_at\", \"instance_configurations\".\"created_by_id\", \"instance_configurations\".\"updated_by_id\", \"instance_configurations\".\"deleted_at\", \"instance_configurations\".\"id\", \"instance_configurations\".\"key\", \"instance_configurations\".\"value\", \"instance_configurations\".\"category\", \"instance_configurations\".\"is_encrypted\" FROM \"instance_configurations\" LIMIT 0",
    columns: INSTANCE_CONFIGURATIONS_COLUMNS,
};

/// Every generated model statement, in `meta::MODELS` order.
pub static ALL_MODEL_QUERIES: &[&Query] = &[
    &ANALYTIC_VIEWS_QUERY,
    &API_ACTIVITY_LOGS_QUERY,
    &API_TOKENS_QUERY,
    &FILE_ASSETS_QUERY,
    &CYCLES_QUERY,
    &CYCLE_ISSUES_QUERY,
    &CYCLE_USER_PROPERTIES_QUERY,
    &DEPLOY_BOARDS_QUERY,
    &DESCRIPTIONS_QUERY,
    &DESCRIPTION_VERSIONS_QUERY,
    &DEVICES_QUERY,
    &DEVICE_SESSIONS_QUERY,
    &DRAFT_ISSUES_QUERY,
    &DRAFT_ISSUE_ASSIGNEES_QUERY,
    &DRAFT_ISSUE_CYCLES_QUERY,
    &DRAFT_ISSUE_LABELS_QUERY,
    &DRAFT_ISSUE_MODULES_QUERY,
    &ESTIMATES_QUERY,
    &ESTIMATE_POINTS_QUERY,
    &EXPORTERS_QUERY,
    &USER_FAVORITES_QUERY,
    &IMPORTERS_QUERY,
    &INTAKES_QUERY,
    &INTAKE_ISSUES_QUERY,
    &INTEGRATIONS_QUERY,
    &WORKSPACE_INTEGRATIONS_QUERY,
    &GITHUB_COMMENT_SYNCS_QUERY,
    &GITHUB_ISSUE_SYNCS_QUERY,
    &GITHUB_REPOSITORIES_QUERY,
    &GITHUB_REPOSITORY_SYNCS_QUERY,
    &SLACK_PROJECT_SYNCS_QUERY,
    &COMMENT_REACTIONS_QUERY,
    &ISSUES_QUERY,
    &ISSUE_ACTIVITIES_QUERY,
    &ISSUE_ASSIGNEES_QUERY,
    &ISSUE_ATTACHMENTS_QUERY,
    &ISSUE_BLOCKERS_QUERY,
    &ISSUE_COMMENTS_QUERY,
    &ISSUE_DESCRIPTION_VERSIONS_QUERY,
    &ISSUE_LABELS_QUERY,
    &ISSUE_LINKS_QUERY,
    &ISSUE_MENTIONS_QUERY,
    &ISSUE_REACTIONS_QUERY,
    &ISSUE_RELATIONS_QUERY,
    &ISSUE_SEQUENCES_QUERY,
    &ISSUE_SUBSCRIBERS_QUERY,
    &ISSUE_VERSIONS_QUERY,
    &ISSUE_VOTES_QUERY,
    &ISSUE_TYPES_QUERY,
    &PROJECT_ISSUE_TYPES_QUERY,
    &LABELS_QUERY,
    &MODULES_QUERY,
    &MODULE_ISSUES_QUERY,
    &MODULE_LINKS_QUERY,
    &MODULE_MEMBERS_QUERY,
    &MODULE_USER_PROPERTIES_QUERY,
    &EMAIL_NOTIFICATION_LOGS_QUERY,
    &NOTIFICATIONS_QUERY,
    &USER_NOTIFICATION_PREFERENCES_QUERY,
    &PAGES_QUERY,
    &PAGE_LABELS_QUERY,
    &PAGE_LOGS_QUERY,
    &PAGE_VERSIONS_QUERY,
    &PROJECT_PAGES_QUERY,
    &PROJECTS_QUERY,
    &PROJECT_DEPLOY_BOARDS_QUERY,
    &PROJECT_IDENTIFIERS_QUERY,
    &PROJECT_MEMBERS_QUERY,
    &PROJECT_MEMBER_INVITES_QUERY,
    &PROJECT_PUBLIC_MEMBERS_QUERY,
    &PROJECT_USER_PROPERTIES_QUERY,
    &USER_RECENT_VISITS_QUERY,
    &SESSIONS_QUERY,
    &SOCIAL_LOGIN_CONNECTIONS_QUERY,
    &STATES_QUERY,
    &STICKIES_QUERY,
    &ACCOUNTS_QUERY,
    &PROFILES_QUERY,
    &USERS_QUERY,
    &USERS_GROUPS_QUERY,
    &USERS_USER_PERMISSIONS_QUERY,
    &ISSUE_VIEWS_QUERY,
    &PROJECT_WEBHOOKS_QUERY,
    &WEBHOOKS_QUERY,
    &WEBHOOK_LOGS_QUERY,
    &TEAMS_QUERY,
    &WORKSPACES_QUERY,
    &WORKSPACE_HOME_PREFERENCES_QUERY,
    &WORKSPACE_MEMBERS_QUERY,
    &WORKSPACE_MEMBER_INVITES_QUERY,
    &WORKSPACE_THEMES_QUERY,
    &WORKSPACE_USER_LINKS_QUERY,
    &WORKSPACE_USER_PREFERENCES_QUERY,
    &WORKSPACE_USER_PROPERTIES_QUERY,
    &CHANGELOGS_QUERY,
    &INSTANCES_QUERY,
    &INSTANCE_ADMINS_QUERY,
    &INSTANCE_CONFIGURATIONS_QUERY,
];
