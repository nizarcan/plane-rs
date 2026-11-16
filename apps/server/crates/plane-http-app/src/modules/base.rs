// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/module/base.py`.
//!
//! * `ModuleViewSet.list` / `retrieve` — `GET …/modules/` and
//!   `GET …/modules/{pk}/`;
//! * `ModuleLinkViewSet.list` / `retrieve` — `GET …/modules/{module_id}/module-links/`
//!   and `GET …/modules/{module_id}/module-links/{pk}/`;
//! * `ModuleUserPropertiesEndpoint.get`;
//! * `ModuleFavoriteViewSet`'s `GET` (a 500: the viewset has no
//!   `serializer_class`, so DRF's `ListModelMixin` fails at `get_serializer`).
//!
//! The list body is a `.values()` dict whose datetimes
//! `user_timezone_converter` renders in the request user's profile timezone
//! (`base.py:393`); with `?fields=` the view switches to
//! `ModuleSerializer(queryset, many=True)` instead, which is the same data in
//! the serializer's field order. The detail body is `ModuleDetailSerializer`
//! plus the two distribution dicts.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_module::{
    ModuleLinkSerializer, ModuleSerializer, ModuleUserPropertiesSerializer,
};
use plane_serializers::{
    DrfDateTime, DrfError, DrfSerialize, DrfUuid, DrfUuidList, DrfWriter, FieldValue, PgJson,
    RenderCtx,
};
use tokio_postgres::Row;
use uuid::Uuid;

use crate::common::{client, jobs};
use crate::cycles::base::{
    self as cycle_base, Chart, ChartKind, DistributionGroup, DistributionKind, Params, date_chart,
};
use crate::modules::queries;

/// The URL kwargs and user id the module routes read.
pub(crate) fn params(ctx: &ViewCtx<'_>) -> Result<Params, ViewError> {
    cycle_base::params(ctx)
}

pub(crate) fn uuid_kwarg(ctx: &ViewCtx<'_>, name: &'static str) -> Result<Uuid, ViewError> {
    cycle_base::uuid_kwarg(ctx, name)
}

fn user_timezone(ctx: &ViewCtx<'_>) -> chrono_tz::Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

fn opt_json(row: &Row, index: usize) -> Result<Option<JsonText>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

fn opt_text(row: &Row, index: usize) -> Result<Option<String>, ViewError> {
    row.try_get(index).map_err(ViewError::from)
}

/// One module row: the union of the columns the five module statements select.
#[derive(Debug, Clone)]
pub(crate) struct ModuleRow {
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) description_text: Option<JsonText>,
    pub(crate) description_html: Option<JsonText>,
    pub(crate) start_date: Option<NaiveDate>,
    pub(crate) target_date: Option<NaiveDate>,
    pub(crate) status: String,
    pub(crate) lead_id: Option<Uuid>,
    pub(crate) member_ids: Vec<Uuid>,
    pub(crate) view_props: Option<JsonText>,
    pub(crate) sort_order: f64,
    pub(crate) external_source: Option<String>,
    pub(crate) external_id: Option<String>,
    pub(crate) logo_props: Option<JsonText>,
    pub(crate) sub_issues: Option<i64>,
    pub(crate) backlog_estimate_points: Option<f64>,
    pub(crate) unstarted_estimate_points: Option<f64>,
    pub(crate) started_estimate_points: Option<f64>,
    pub(crate) cancelled_estimate_points: Option<f64>,
    pub(crate) completed_estimate_points: Option<f64>,
    pub(crate) total_estimate_points: Option<f64>,
    pub(crate) is_favorite: Option<bool>,
    pub(crate) total_issues: i64,
    pub(crate) cancelled_issues: Option<i64>,
    pub(crate) completed_issues: Option<i64>,
    pub(crate) started_issues: Option<i64>,
    pub(crate) unstarted_issues: Option<i64>,
    pub(crate) backlog_issues: Option<i64>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub(crate) struct ModuleLinkRow {
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) created_by_id: Option<Uuid>,
    pub(crate) updated_by_id: Option<Uuid>,
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    pub(crate) id: Uuid,
    pub(crate) project_id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) title: Option<String>,
    pub(crate) url: String,
    pub(crate) module_id: Uuid,
    pub(crate) metadata: Option<JsonText>,
}

impl ModuleLinkRow {
    pub(crate) fn decode(row: &Row) -> Result<Self, ViewError> {
        Ok(Self {
            created_at: row.try_get(0).map_err(ViewError::from)?,
            updated_at: row.try_get(1).map_err(ViewError::from)?,
            created_by_id: row.try_get(2).map_err(ViewError::from)?,
            updated_by_id: row.try_get(3).map_err(ViewError::from)?,
            deleted_at: row.try_get(4).map_err(ViewError::from)?,
            id: row.try_get(5).map_err(ViewError::from)?,
            project_id: row.try_get(6).map_err(ViewError::from)?,
            workspace_id: row.try_get(7).map_err(ViewError::from)?,
            title: opt_text(row, 8)?,
            url: row.try_get(9).map_err(ViewError::from)?,
            module_id: row.try_get(10).map_err(ViewError::from)?,
            metadata: opt_json(row, 11)?,
        })
    }

    pub(crate) fn value(&self) -> ModuleLinkSerializer<'_> {
        ModuleLinkSerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            title: self.title.as_deref(),
            url: &self.url,
            metadata: self.metadata.as_ref().map(PgJson::from),
            created_by: self.created_by_id,
            updated_by: self.updated_by_id,
            project: self.project_id,
            workspace: self.workspace_id,
            module: self.module_id,
        }
    }
}

/// `ModuleViewSet.list`'s `.values()` row (`module/base.py:360-391`).
pub(crate) fn decode_list_row(row: &Row) -> Result<ModuleRow, ViewError> {
    Ok(ModuleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: row.try_get(3).map_err(ViewError::from)?,
        description: row.try_get(4).map_err(ViewError::from)?,
        description_text: opt_json(row, 5)?,
        description_html: opt_json(row, 6)?,
        start_date: row.try_get(7).map_err(ViewError::from)?,
        target_date: row.try_get(8).map_err(ViewError::from)?,
        status: row.try_get(9).map_err(ViewError::from)?,
        lead_id: row.try_get(10).map_err(ViewError::from)?,
        member_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(11)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        view_props: opt_json(row, 12)?,
        sort_order: row.try_get(13).map_err(ViewError::from)?,
        external_source: opt_text(row, 14)?,
        external_id: opt_text(row, 15)?,
        logo_props: opt_json(row, 16)?,
        sub_issues: None,
        backlog_estimate_points: None,
        unstarted_estimate_points: None,
        started_estimate_points: None,
        cancelled_estimate_points: None,
        completed_estimate_points: row.try_get(17).map_err(ViewError::from)?,
        total_estimate_points: row.try_get(18).map_err(ViewError::from)?,
        is_favorite: Some(row.try_get(20).map_err(ViewError::from)?),
        total_issues: row.try_get(19).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(21).map_err(ViewError::from)?,
        completed_issues: row.try_get(22).map_err(ViewError::from)?,
        started_issues: row.try_get(23).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(24).map_err(ViewError::from)?,
        backlog_issues: row.try_get(25).map_err(ViewError::from)?,
        created_at: row.try_get(26).map_err(ViewError::from)?,
        updated_at: row.try_get(27).map_err(ViewError::from)?,
        archived_at: None,
    })
}

/// `ModuleViewSet.retrieve`'s row (`module/base.py:398-413`).
pub(crate) fn decode_retrieve_row(row: &Row) -> Result<ModuleRow, ViewError> {
    Ok(ModuleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: row.try_get(3).map_err(ViewError::from)?,
        description: row.try_get(4).map_err(ViewError::from)?,
        description_text: opt_json(row, 5)?,
        description_html: opt_json(row, 6)?,
        start_date: row.try_get(7).map_err(ViewError::from)?,
        target_date: row.try_get(8).map_err(ViewError::from)?,
        status: row.try_get(9).map_err(ViewError::from)?,
        lead_id: row.try_get(10).map_err(ViewError::from)?,
        member_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(11)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        view_props: opt_json(row, 12)?,
        sort_order: row.try_get(13).map_err(ViewError::from)?,
        external_source: opt_text(row, 14)?,
        external_id: opt_text(row, 15)?,
        logo_props: opt_json(row, 16)?,
        sub_issues: Some(row.try_get(17).map_err(ViewError::from)?),
        backlog_estimate_points: row.try_get(18).map_err(ViewError::from)?,
        unstarted_estimate_points: row.try_get(19).map_err(ViewError::from)?,
        started_estimate_points: row.try_get(20).map_err(ViewError::from)?,
        cancelled_estimate_points: row.try_get(21).map_err(ViewError::from)?,
        completed_estimate_points: row.try_get(22).map_err(ViewError::from)?,
        total_estimate_points: row.try_get(23).map_err(ViewError::from)?,
        is_favorite: Some(row.try_get(24).map_err(ViewError::from)?),
        total_issues: row.try_get(25).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(26).map_err(ViewError::from)?,
        completed_issues: row.try_get(27).map_err(ViewError::from)?,
        started_issues: row.try_get(28).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(29).map_err(ViewError::from)?,
        backlog_issues: row.try_get(30).map_err(ViewError::from)?,
        created_at: row.try_get(31).map_err(ViewError::from)?,
        updated_at: row.try_get(32).map_err(ViewError::from)?,
        archived_at: row.try_get(33).map_err(ViewError::from)?,
    })
}

/// `ModuleArchiveUnarchiveEndpoint.get`'s archived list row
/// (`module/archive.py:261-290`).
pub(crate) fn decode_archived_list_row(row: &Row) -> Result<ModuleRow, ViewError> {
    Ok(ModuleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: row.try_get(3).map_err(ViewError::from)?,
        description: row.try_get(4).map_err(ViewError::from)?,
        description_text: opt_json(row, 5)?,
        description_html: opt_json(row, 6)?,
        start_date: row.try_get(7).map_err(ViewError::from)?,
        target_date: row.try_get(8).map_err(ViewError::from)?,
        status: row.try_get(9).map_err(ViewError::from)?,
        lead_id: row.try_get(10).map_err(ViewError::from)?,
        member_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(11)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        view_props: opt_json(row, 12)?,
        sort_order: row.try_get(13).map_err(ViewError::from)?,
        external_source: opt_text(row, 14)?,
        external_id: opt_text(row, 15)?,
        logo_props: None,
        sub_issues: None,
        backlog_estimate_points: None,
        unstarted_estimate_points: None,
        started_estimate_points: None,
        cancelled_estimate_points: None,
        completed_estimate_points: None,
        total_estimate_points: None,
        is_favorite: Some(row.try_get(17).map_err(ViewError::from)?),
        total_issues: row.try_get(16).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(18).map_err(ViewError::from)?,
        completed_issues: row.try_get(19).map_err(ViewError::from)?,
        started_issues: row.try_get(20).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(21).map_err(ViewError::from)?,
        backlog_issues: row.try_get(22).map_err(ViewError::from)?,
        created_at: row.try_get(23).map_err(ViewError::from)?,
        updated_at: row.try_get(24).map_err(ViewError::from)?,
        archived_at: row.try_get(25).map_err(ViewError::from)?,
    })
}

/// `WorkspaceModulesEndpoint`'s row (`workspace/module.py:26-129`).
pub(crate) fn decode_workspace_row(row: &Row) -> Result<ModuleRow, ViewError> {
    Ok(ModuleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: row.try_get(3).map_err(ViewError::from)?,
        description: row.try_get(4).map_err(ViewError::from)?,
        description_text: opt_json(row, 5)?,
        description_html: opt_json(row, 6)?,
        start_date: row.try_get(7).map_err(ViewError::from)?,
        target_date: row.try_get(8).map_err(ViewError::from)?,
        status: row.try_get(9).map_err(ViewError::from)?,
        lead_id: row.try_get(10).map_err(ViewError::from)?,
        member_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(11)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        view_props: opt_json(row, 12)?,
        sort_order: row.try_get(13).map_err(ViewError::from)?,
        external_source: opt_text(row, 14)?,
        external_id: opt_text(row, 15)?,
        logo_props: opt_json(row, 16)?,
        sub_issues: None,
        backlog_estimate_points: None,
        unstarted_estimate_points: None,
        started_estimate_points: None,
        cancelled_estimate_points: None,
        completed_estimate_points: None,
        total_estimate_points: None,
        is_favorite: None,
        total_issues: row.try_get(17).map_err(ViewError::from)?,
        cancelled_issues: row.try_get(18).map_err(ViewError::from)?,
        completed_issues: row.try_get(19).map_err(ViewError::from)?,
        started_issues: row.try_get(20).map_err(ViewError::from)?,
        unstarted_issues: row.try_get(21).map_err(ViewError::from)?,
        backlog_issues: row.try_get(22).map_err(ViewError::from)?,
        created_at: row.try_get(23).map_err(ViewError::from)?,
        updated_at: row.try_get(24).map_err(ViewError::from)?,
        archived_at: row.try_get(25).map_err(ViewError::from)?,
    })
}

/// Which annotations a Python queryset adds before `ModuleSerializer` runs.
///
/// `ModuleSerializer` declares the estimate-point sums and `is_favorite` as
/// `required=False` fields. DRF writes `null` when the attribute exists and is
/// `None`, and omits the key when the attribute is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModuleAnnotations {
    /// `ModuleViewSet.get_queryset` (`plane/app/views/module/base.py:79-294`)
    /// annotates every field, so the attributes always exist.
    Project,
    /// `WorkspaceModulesEndpoint.get` (`plane/app/views/workspace/module.py:25-160`)
    /// annotates the issue counts and `member_ids` only; the estimate-point
    /// sums and `is_favorite` are missing, so DRF skips those keys.
    Workspace,
}

impl ModuleRow {
    /// `ModuleSerializer`'s data (`SERIALIZERS.md`), for the `?fields=` list
    /// path and the workspace list.
    pub(crate) fn serializer(&self, annotations: ModuleAnnotations) -> ModuleSerializer<'_> {
        let (total_estimate_points, completed_estimate_points, is_favorite) = match annotations {
            ModuleAnnotations::Project => (
                FieldValue::from_attribute(self.total_estimate_points),
                FieldValue::from_attribute(self.completed_estimate_points),
                FieldValue::from_attribute(self.is_favorite),
            ),
            ModuleAnnotations::Workspace => (
                FieldValue::absent(),
                FieldValue::absent(),
                FieldValue::absent(),
            ),
        };
        ModuleSerializer {
            id: self.id,
            workspace_id: self.workspace_id,
            project_id: self.project_id,
            name: &self.name,
            description: &self.description,
            description_text: self.description_text.as_ref().map(PgJson::from),
            description_html: self.description_html.as_ref().map(PgJson::from),
            start_date: self.start_date,
            target_date: self.target_date,
            status: &self.status,
            lead_id: self.lead_id,
            member_ids: Some(self.member_ids.as_slice()),
            view_props: self.view_props.as_ref().map(PgJson::from),
            sort_order: self.sort_order,
            external_source: self.external_source.as_deref(),
            external_id: self.external_id.as_deref(),
            logo_props: self.logo_props.as_ref().map(PgJson::from),
            // The issue counts and `member_ids` are annotated by both views
            // (`module/base.py:214-294`, `workspace/module.py:44-129`); a NULL
            // is DRF's `null` (`rest_framework/serializers.py:541-546`).
            total_estimate_points,
            completed_estimate_points,
            is_favorite,
            total_issues: FieldValue::value(self.total_issues),
            cancelled_issues: FieldValue::from_attribute(self.cancelled_issues),
            completed_issues: FieldValue::from_attribute(self.completed_issues),
            started_issues: FieldValue::from_attribute(self.started_issues),
            unstarted_issues: FieldValue::from_attribute(self.unstarted_issues),
            backlog_issues: FieldValue::from_attribute(self.backlog_issues),
            created_at: self.created_at,
            updated_at: self.updated_at,
            archived_at: self.archived_at,
        }
    }
}

/// `ModuleViewSet.list`'s `.values()` projection (`module/base.py:360-391`).
#[derive(Debug)]
pub(crate) struct ModuleListValue<'a>(pub(crate) &'a ModuleRow);

impl DrfSerialize for ModuleListValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let timezone = *ctx.tz();
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
        obj.field("name", &row.name)?;
        obj.field("description", &row.description)?;
        obj.field_json(
            "description_text",
            row.description_text.as_ref().map(PgJson::from),
        )?;
        obj.field_json(
            "description_html",
            row.description_html.as_ref().map(PgJson::from),
        )?;
        obj.field("start_date", &row.start_date)?;
        obj.field("target_date", &row.target_date)?;
        obj.field("status", &row.status)?;
        obj.field("lead_id", &row.lead_id)?;
        obj.field("member_ids", &DrfUuidList(&row.member_ids))?;
        obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &row.sort_order)?;
        obj.field("external_source", &row.external_source)?;
        obj.field("external_id", &row.external_id)?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field(
            "completed_estimate_points",
            &row.completed_estimate_points.unwrap_or_default(),
        )?;
        obj.field(
            "total_estimate_points",
            &row.total_estimate_points.unwrap_or_default(),
        )?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("is_favorite", &row.is_favorite.unwrap_or_default())?;
        obj.field(
            "cancelled_issues",
            &row.cancelled_issues.unwrap_or_default(),
        )?;
        obj.field(
            "completed_issues",
            &row.completed_issues.unwrap_or_default(),
        )?;
        obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
        obj.field(
            "unstarted_issues",
            &row.unstarted_issues.unwrap_or_default(),
        )?;
        obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
        obj.field(
            "created_at",
            &DrfDateTime(row.created_at.with_timezone(&timezone)),
        )?;
        obj.field(
            "updated_at",
            &DrfDateTime(row.updated_at.with_timezone(&timezone)),
        )?;
        obj.end();
        Ok(())
    }
}

/// `ModuleArchiveUnarchiveEndpoint.get`'s archived list projection
/// (`module/archive.py:261-290`): no `logo_props`, `archived_at` last.
#[derive(Debug)]
pub(crate) struct ModuleArchivedListValue<'a>(pub(crate) &'a ModuleRow);

impl DrfSerialize for ModuleArchivedListValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let timezone = *ctx.tz();
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
        obj.field("name", &row.name)?;
        obj.field("description", &row.description)?;
        obj.field_json(
            "description_text",
            row.description_text.as_ref().map(PgJson::from),
        )?;
        obj.field_json(
            "description_html",
            row.description_html.as_ref().map(PgJson::from),
        )?;
        obj.field("start_date", &row.start_date)?;
        obj.field("target_date", &row.target_date)?;
        obj.field("status", &row.status)?;
        obj.field("lead_id", &row.lead_id)?;
        obj.field("member_ids", &DrfUuidList(&row.member_ids))?;
        obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &row.sort_order)?;
        obj.field("external_source", &row.external_source)?;
        obj.field("external_id", &row.external_id)?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("is_favorite", &row.is_favorite.unwrap_or_default())?;
        obj.field(
            "cancelled_issues",
            &row.cancelled_issues.unwrap_or_default(),
        )?;
        obj.field(
            "completed_issues",
            &row.completed_issues.unwrap_or_default(),
        )?;
        obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
        obj.field(
            "unstarted_issues",
            &row.unstarted_issues.unwrap_or_default(),
        )?;
        obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
        obj.field(
            "created_at",
            &DrfDateTime(row.created_at.with_timezone(&timezone)),
        )?;
        obj.field(
            "updated_at",
            &DrfDateTime(row.updated_at.with_timezone(&timezone)),
        )?;
        obj.field("archived_at", &row.archived_at)?;
        obj.end();
        Ok(())
    }
}

/// `ModuleViewSet.list` (`module/base.py:354-394`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let timezone = user_timezone(ctx);
        let statement = client
            .prepare_cached(queries::MODULE_LIST.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[&params.slug, &params.project_id, &params.user_id],
            )
            .await
            .map_err(ViewError::from)?;
        let decoded = rows
            .iter()
            .map(decode_list_row)
            .collect::<Result<Vec<_>, _>>()?;
        if ctx.fields().is_some() {
            // `ModuleSerializer(queryset, many=True, fields=self.fields)`:
            // the view passes `fields` and *not* `expand`, so `?expand=` is
            // ignored on this route (the values path ignores it too).
            let timezone = user_timezone(ctx);
            let fields = ctx.fields().map(|names| {
                names
                    .into_iter()
                    .map(plane_serializers::FieldEntry::name)
                    .collect::<Vec<_>>()
            });
            let render_ctx = RenderCtx::new(&timezone, fields.as_deref(), &[]);
            let serializers: Vec<ModuleSerializer<'_>> = decoded
                .iter()
                .map(|row| row.serializer(ModuleAnnotations::Project))
                .collect();
            let bytes = plane_serializers::render_list(&serializers, &render_ctx)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes));
        }
        let values: Vec<ModuleListValue<'_>> = decoded.iter().map(ModuleListValue).collect();
        cycle_base::render_list(&values, timezone)
    })
}

/// `ModuleViewSet.retrieve` (`module/base.py:396-650`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let timezone = user_timezone(ctx);
        let rows = fetch(
            client,
            &queries::MODULE_RETRIEVE,
            &[&params.slug, &params.project_id, &params.user_id, &pk],
        )
        .await?;
        let Some(row) = rows.first() else {
            return Ok(cycle_base::error_body(404, "Module not found"));
        };
        let module = decode_retrieve_row(row)?;
        let links = fetch(client, &queries::MODULE_LINKS_BY_MODULE, &[&pk]).await?;
        let links = links
            .iter()
            .map(ModuleLinkRow::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let estimate_type = estimate_type_exists(client, &params).await?;

        let mut estimate_assignees = Vec::new();
        let mut estimate_labels = Vec::new();
        let mut estimate_chart: Option<Chart> = None;
        if estimate_type {
            estimate_assignees = cycle_base::distribute(
                client,
                &queries::MODULE_ASSIGNEES_POINTS,
                &[&pk, &params.project_id, &params.slug],
                DistributionKind::ModuleAssigneesPoints,
            )
            .await?;
            estimate_labels = cycle_base::distribute(
                client,
                &queries::MODULE_LABELS_POINTS,
                &[&pk, &params.project_id, &params.slug],
                DistributionKind::ArchiveLabelsPoints,
            )
            .await?;
            if let (Some(start), Some(target)) = (module.start_date, module.target_date) {
                estimate_chart =
                    Some(points_chart(client, &params, pk, start, target, timezone).await?);
            }
        }

        let assignees = cycle_base::distribute(
            client,
            &queries::MODULE_ASSIGNEES_ISSUES,
            &[&pk, &params.project_id, &params.slug],
            DistributionKind::ArchiveAssigneesIssues,
        )
        .await?;
        let labels = cycle_base::distribute(
            client,
            &queries::MODULE_LABELS_ISSUES,
            &[&pk, &params.project_id, &params.slug],
            DistributionKind::ArchiveLabelsIssues,
        )
        .await?;
        let mut chart = Chart(Vec::new());
        if let (Some(start), Some(target)) = (module.start_date, module.target_date)
            && module.total_issues > 0
        {
            chart = issues_chart(
                client,
                &params,
                pk,
                start,
                target,
                module.total_issues,
                timezone,
            )
            .await?;
        }

        let _ = jobs::recent_visited_entity(
            ctx,
            "module",
            plane_jobs::celery::args::TaskArg::uuid(pk),
            Some(plane_jobs::celery::args::TaskArg::uuid(params.project_id)),
            plane_jobs::celery::args::TaskArg::uuid(params.user_id),
        )
        .await?;

        let body = ModuleDetailBody {
            module,
            links: &links,
            estimate: estimate_type.then_some(DistributionGroup {
                assignees: &estimate_assignees,
                labels: &estimate_labels,
                chart: estimate_chart.as_ref(),
            }),
            distribution: DistributionGroup {
                assignees: &assignees,
                labels: &labels,
                chart: Some(&chart),
            },
        };
        let bytes = plane_serializers::render(&body, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

pub(crate) async fn fetch(
    client: &Client,
    query: &plane_db::Query,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<Row>, ViewError> {
    let statement = client
        .prepare_cached(query.sql)
        .await
        .map_err(ViewError::from)?;
    client
        .query(&statement, params)
        .await
        .map_err(ViewError::from)
}

/// The `estimate_type` guard of the module detail/archive
/// (`module/base.py:418-423`).
pub(crate) async fn estimate_type_exists(
    client: &Client,
    params: &Params,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::MODULE_ESTIMATE_TYPE_EXISTS.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_one(&statement, &[&params.project_id, &params.slug])
        .await
        .map_err(ViewError::from)?;
    row.try_get(0).map_err(ViewError::from)
}

/// `ModuleDetailSerializer`'s 35 fields followed by `estimate_distribution`
/// and `distribution` (`module/base.py:425-650`).
#[derive(Debug)]
pub(crate) struct ModuleDetailBody<'a> {
    pub(crate) module: ModuleRow,
    pub(crate) links: &'a [ModuleLinkRow],
    pub(crate) estimate: Option<DistributionGroup<'a>>,
    pub(crate) distribution: DistributionGroup<'a>,
}

impl DrfSerialize for ModuleDetailBody<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = &self.module;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
        obj.field("name", &row.name)?;
        obj.field("description", &row.description)?;
        obj.field_json(
            "description_text",
            row.description_text.as_ref().map(PgJson::from),
        )?;
        obj.field_json(
            "description_html",
            row.description_html.as_ref().map(PgJson::from),
        )?;
        obj.field("start_date", &row.start_date)?;
        obj.field("target_date", &row.target_date)?;
        obj.field("status", &row.status)?;
        obj.field("lead_id", &row.lead_id)?;
        obj.field("member_ids", &DrfUuidList(&row.member_ids))?;
        obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &row.sort_order)?;
        obj.field("external_source", &row.external_source)?;
        obj.field("external_id", &row.external_id)?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field(
            "total_estimate_points",
            &row.total_estimate_points.unwrap_or_default(),
        )?;
        obj.field(
            "completed_estimate_points",
            &row.completed_estimate_points.unwrap_or_default(),
        )?;
        obj.field("is_favorite", &row.is_favorite.unwrap_or_default())?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field(
            "cancelled_issues",
            &row.cancelled_issues.unwrap_or_default(),
        )?;
        obj.field(
            "completed_issues",
            &row.completed_issues.unwrap_or_default(),
        )?;
        obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
        obj.field(
            "unstarted_issues",
            &row.unstarted_issues.unwrap_or_default(),
        )?;
        obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
        obj.field(
            "created_at",
            &DrfDateTime(row.created_at.with_timezone(ctx.tz())),
        )?;
        obj.field(
            "updated_at",
            &DrfDateTime(row.updated_at.with_timezone(ctx.tz())),
        )?;
        obj.field(
            "archived_at",
            &row.archived_at
                .map(|value| DrfDateTime(value.with_timezone(ctx.tz()))),
        )?;
        obj.field_nested_list_dyn(
            "link_module",
            ctx,
            &self
                .links
                .iter()
                .map(ModuleLinkRow::value)
                .collect::<Vec<_>>(),
        )?;
        obj.field("sub_issues", &row.sub_issues.unwrap_or_default())?;
        obj.field(
            "backlog_estimate_points",
            &row.backlog_estimate_points.unwrap_or_default(),
        )?;
        obj.field(
            "unstarted_estimate_points",
            &row.unstarted_estimate_points.unwrap_or_default(),
        )?;
        obj.field(
            "started_estimate_points",
            &row.started_estimate_points.unwrap_or_default(),
        )?;
        obj.field(
            "cancelled_estimate_points",
            &row.cancelled_estimate_points.unwrap_or_default(),
        )?;
        match &self.estimate {
            Some(estimate) => obj.field_nested_dyn("estimate_distribution", ctx, Some(estimate))?,
            None => obj.field(
                "estimate_distribution",
                &std::collections::BTreeMap::<String, String>::new(),
            )?,
        }
        obj.field_nested_dyn("distribution", ctx, Some(&self.distribution))?;
        obj.end();
        Ok(())
    }
}

/// `ModuleLinkViewSet.list` (`module/base.py:781-795`, `ListModelMixin`): a
/// bare array of serialized links.
pub fn links_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let rows = fetch(
            client,
            &queries::MODULE_LINKS_LIST,
            &[
                &params.slug,
                &params.project_id,
                &module_id,
                &params.user_id,
            ],
        )
        .await?;
        let links = rows
            .iter()
            .map(ModuleLinkRow::decode)
            .collect::<Result<Vec<_>, _>>()?;
        let timezone = user_timezone(ctx);
        let values: Vec<ModuleLinkSerializer<'_>> =
            links.iter().map(ModuleLinkRow::value).collect();
        let bytes = plane_serializers::render_list(&values, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `ModuleLinkViewSet.retrieve` (`module/base.py:781-795` + `RetrieveModelMixin`).
pub fn links_retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let rows = fetch(
            client,
            &queries::MODULE_LINK_BY_PK,
            &[
                &params.slug,
                &params.project_id,
                &module_id,
                &params.user_id,
                &pk,
            ],
        )
        .await?;
        let Some(row) = rows.first() else {
            // `RetrieveModelMixin.get_object` is `get_object_or_404`, whose
            // `Http404` carries the model's DoesNotExist message.
            return Ok(ViewResponse::json(
                404,
                serde_json::json!({
                    "detail": "No ModuleLink matches the given query."
                }),
            ));
        };
        let link = ModuleLinkRow::decode(row)?;
        let timezone = user_timezone(ctx);
        let value = link.value();
        let bytes = plane_serializers::render(&value, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `ModuleUserPropertiesEndpoint.get` (`module/base.py:857-865`).
pub fn user_properties_get<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let statement = client
            .prepare_cached(queries::MODULE_USER_PROPERTIES_BY_KEY.sql)
            .await
            .map_err(ViewError::from)?;
        let row = client
            .query_opt(
                &statement,
                &[
                    &params.user_id,
                    &params.project_id,
                    &module_id,
                    &params.slug,
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let mut created = false;
        let row = match row {
            Some(row) => row,
            None => {
                created = true;
                let insert = client
                    .prepare_cached(queries::MODULE_USER_PROPERTIES_INSERT.sql)
                    .await
                    .map_err(ViewError::from)?;
                let now = Utc::now();
                let id = Uuid::new_v4();
                match client
                    .query_one(
                        &insert,
                        &[&now, &params.user_id, &id, &params.project_id, &module_id],
                    )
                    .await
                {
                    Ok(row) => row,
                    Err(error)
                        if error.code()
                            == Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION) =>
                    {
                        return Err(ViewError::Integrity);
                    }
                    Err(error) => return Err(ViewError::from(error)),
                }
            }
        };
        // A row Django inserts is rendered from the in-memory default dicts,
        // which keep the Python literal key order; a stored row renders its
        // `jsonb` order (see `DEFAULT_FILTERS`).
        let filters = if created {
            Some(cycle_base::DEFAULT_FILTERS.to_string())
        } else {
            opt_json(&row, 10)?.map(JsonText::into_string)
        };
        let display_filters = if created {
            Some(cycle_base::DEFAULT_DISPLAY_FILTERS.to_string())
        } else {
            opt_json(&row, 11)?.map(JsonText::into_string)
        };
        let display_properties = if created {
            Some(cycle_base::DEFAULT_DISPLAY_PROPERTIES.to_string())
        } else {
            opt_json(&row, 12)?.map(JsonText::into_string)
        };
        let rich_filters = if created {
            Some("{}".to_string())
        } else {
            opt_json(&row, 13)?.map(JsonText::into_string)
        };
        let serializer = ModuleUserPropertiesSerializer {
            id: row.try_get(5).map_err(ViewError::from)?,
            created_at: row.try_get(0).map_err(ViewError::from)?,
            updated_at: row.try_get(1).map_err(ViewError::from)?,
            deleted_at: row.try_get(4).map_err(ViewError::from)?,
            filters: filters.as_deref().map(PgJson::new),
            display_filters: display_filters.as_deref().map(PgJson::new),
            display_properties: display_properties.as_deref().map(PgJson::new),
            rich_filters: rich_filters.as_deref().map(PgJson::new),
            created_by: row.try_get(2).map_err(ViewError::from)?,
            updated_by: row.try_get(3).map_err(ViewError::from)?,
            project: row.try_get(6).map_err(ViewError::from)?,
            workspace: row.try_get(7).map_err(ViewError::from)?,
            module: row.try_get(8).map_err(ViewError::from)?,
            user: row.try_get(9).map_err(ViewError::from)?,
        };
        let timezone = user_timezone(ctx);
        let bytes = plane_serializers::render(&serializer, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
    })
}

/// `ModuleFavoriteViewSet`'s `GET`: the router maps `list` to
/// `ListModelMixin`, the viewset has no `serializer_class`, and
/// `GenericAPIView.get_serializer` raises `AssertionError` — Django's family
/// handler answers 500 (`module/base.py:798-833`).
pub fn favorites_list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let _ = ctx;
        Err(ViewError::Unknown(
            "AssertionError: serializer_class should be set on ModuleFavoriteViewSet".to_string(),
        ))
    })
}

/// `burndown_plot(plot_type="points")` for a module (`analytics_plot.py:123-265`).
pub(crate) async fn points_chart(
    client: &Client,
    params: &Params,
    module_id: Uuid,
    start: NaiveDate,
    target: NaiveDate,
    timezone: chrono_tz::Tz,
) -> Result<Chart, ViewError> {
    let (total, is_float) = {
        let statement = client
            .prepare_cached(queries::MODULE_BURNDOWN_TOTAL_ESTIMATES.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(&statement, &[&module_id, &params.project_id, &params.slug])
            .await
            .map_err(ViewError::from)?;
        let mut sum = 0.0_f64;
        for row in &rows {
            let value: f64 = row.try_get(0).map_err(ViewError::from)?;
            sum += value;
        }
        (sum, !rows.is_empty())
    };
    let distribution = {
        let statement = client
            .prepare_cached(queries::MODULE_BURNDOWN_COMPLETED_POINTS.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[
                    &module_id,
                    &params.project_id,
                    &params.slug,
                    &timezone.name(),
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let mut out: Vec<(NaiveDate, f64)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let date: Option<NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
            let value: f64 = row.try_get(1).map_err(ViewError::from)?;
            if let Some(date) = date {
                out.push((date, value));
            }
        }
        out
    };
    Ok(date_chart(
        start,
        target,
        ChartKind::Points { total, is_float },
        Some(&distribution),
        None,
    ))
}

/// `burndown_plot(plot_type="issues")` for a module.
pub(crate) async fn issues_chart(
    client: &Client,
    params: &Params,
    module_id: Uuid,
    start: NaiveDate,
    target: NaiveDate,
    total_issues: i64,
    timezone: chrono_tz::Tz,
) -> Result<Chart, ViewError> {
    let distribution = {
        let statement = client
            .prepare_cached(queries::MODULE_BURNDOWN_COMPLETED_ISSUES.sql)
            .await
            .map_err(ViewError::from)?;
        let rows = client
            .query(
                &statement,
                &[
                    &module_id,
                    &params.project_id,
                    &params.slug,
                    &timezone.name(),
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let mut out: Vec<(NaiveDate, i64)> = Vec::with_capacity(rows.len());
        for row in &rows {
            let date: Option<NaiveDate> = row.try_get(0).map_err(ViewError::from)?;
            let count: i64 = row.try_get(1).map_err(ViewError::from)?;
            if let Some(date) = date {
                out.push((date, count));
            }
        }
        out
    };
    Ok(date_chart(
        start,
        target,
        ChartKind::Issues(total_issues),
        None,
        Some(&distribution),
    ))
}
