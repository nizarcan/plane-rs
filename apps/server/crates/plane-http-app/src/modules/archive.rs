// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/module/archive.py` — `ModuleArchiveUnarchiveEndpoint.get`.
//!
//! Three routes, one method:
//!
//! * `GET …/archived-modules/` — the archived list;
//! * `GET …/archived-modules/{pk}/` — one archived module with both
//!   distributions;
//! * `GET …/modules/{module_id}/archive/` — `get(self, request, slug,
//!   project_id, pk=None)` called with `module_id`, a `TypeError` (500) in
//!   Django (`archive.py:258`), reproduced by [`get`].
//!
//! The single-module branch has no existence check: a miss makes
//! `ModuleDetailSerializer(None).data` return `get_initial()`, which is
//! `{"member_ids": []}` plus the two distribution dicts — a 200 with an almost
//! empty body (`archive.py:318-322`).

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, RenderCtx};
use uuid::Uuid;

use crate::cycles::base::{self, Chart, DistributionGroup, render_list};
use crate::modules::base::{
    self as module_base, ModuleArchivedListValue, ModuleDetailBody, ModuleLinkRow,
    decode_archived_list_row, decode_retrieve_row, fetch, issues_chart, points_chart,
};
use crate::modules::queries;

/// `ModuleArchiveUnarchiveEndpoint.get` (`module/archive.py:258-542`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_none() {
            if ctx.kwarg("module_id").is_some() {
                return Err(ViewError::Unknown(
                    "TypeError: ModuleArchiveUnarchiveEndpoint.get() got an unexpected keyword \
                     argument 'module_id'"
                        .to_string(),
                ));
            }
            return list(ctx).await;
        }
        detail(ctx).await
    })
}

/// The archived list (`module/archive.py:258-293`).
async fn list(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let params = module_base::params(ctx)?;
    let rows = fetch(
        client,
        &queries::MODULE_ARCHIVED_LIST,
        &[&params.slug, &params.project_id, &params.user_id],
    )
    .await?;
    let decoded = rows
        .iter()
        .map(decode_archived_list_row)
        .collect::<Result<Vec<_>, _>>()?;
    let values: Vec<ModuleArchivedListValue<'_>> =
        decoded.iter().map(ModuleArchivedListValue).collect();
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    render_list(&values, timezone)
}

/// The single-module branch (`module/archive.py:295-542`).
async fn detail(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    let client = crate::common::client(ctx)?;
    let params = module_base::params(ctx)?;
    let pk = module_base::uuid_kwarg(ctx, "pk")?;
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let rows = fetch(
        client,
        &queries::MODULE_ARCHIVED_RETRIEVE,
        &[&params.slug, &params.project_id, &params.user_id, &pk],
    )
    .await?;
    let module = rows.first().map(decode_retrieve_row).transpose()?;
    let estimate_type = module_base::estimate_type_exists(client, &params).await?;

    let mut estimate_assignees = Vec::new();
    let mut estimate_labels = Vec::new();
    let mut estimate_chart: Option<Chart> = None;
    if estimate_type {
        estimate_assignees = base::distribute(
            client,
            &queries::MODULE_ASSIGNEES_POINTS,
            &[&pk, &params.project_id, &params.slug],
            base::DistributionKind::ModuleAssigneesPoints,
        )
        .await?;
        estimate_labels = base::distribute(
            client,
            &queries::MODULE_LABELS_POINTS,
            &[&pk, &params.project_id, &params.slug],
            base::DistributionKind::ArchiveLabelsPoints,
        )
        .await?;
        if let Some(module) = &module
            && let (Some(start), Some(target)) = (module.start_date, module.target_date)
        {
            estimate_chart =
                Some(points_chart(client, &params, pk, start, target, timezone).await?);
        }
    }

    let assignees = base::distribute(
        client,
        &queries::MODULE_ASSIGNEES_ISSUES,
        &[&pk, &params.project_id, &params.slug],
        base::DistributionKind::ArchiveAssigneesIssues,
    )
    .await?;
    let labels = base::distribute(
        client,
        &queries::MODULE_LABELS_ISSUES,
        &[&pk, &params.project_id, &params.slug],
        base::DistributionKind::ArchiveLabelsIssues,
    )
    .await?;
    let mut chart = Chart(Vec::new());
    if let Some(module) = &module
        && let (Some(start), Some(target)) = (module.start_date, module.target_date)
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

    let distribution = DistributionGroup {
        assignees: &assignees,
        labels: &labels,
        chart: Some(&chart),
    };
    let estimate = estimate_type.then_some(DistributionGroup {
        assignees: &estimate_assignees,
        labels: &estimate_labels,
        chart: estimate_chart.as_ref(),
    });

    let bytes = match module {
        Some(module) => {
            let links = fetch(client, &queries::MODULE_LINKS_BY_MODULE, &[&pk]).await?;
            let links = links
                .iter()
                .map(ModuleLinkRow::decode)
                .collect::<Result<Vec<_>, _>>()?;
            plane_serializers::render(
                &ModuleDetailBody {
                    module,
                    links: &links,
                    estimate,
                    distribution,
                },
                &RenderCtx::full(&timezone),
            )?
        }
        None => plane_serializers::render(
            &EmptyModuleDetail {
                estimate: estimate.is_some(),
                estimate_assignees: &estimate_assignees,
                estimate_labels: &estimate_labels,
                estimate_chart: estimate_chart.as_ref(),
                distribution,
            },
            &RenderCtx::full(&timezone),
        )?,
    };
    Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes))
}

/// `ModuleDetailSerializer(None).data` (`get_initial()`) followed by the two
/// distribution keys.
#[derive(Debug)]
struct EmptyModuleDetail<'a> {
    estimate: bool,
    estimate_assignees: &'a [base::Distribution],
    estimate_labels: &'a [base::Distribution],
    estimate_chart: Option<&'a Chart>,
    distribution: DistributionGroup<'a>,
}

impl DrfSerialize for EmptyModuleDetail<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        // `ListField.get_initial()` is `[]`; every other module field's
        // initial is DRF's `empty` sentinel and is skipped.
        obj.field("member_ids", &Vec::<Uuid>::new())?;
        if self.estimate {
            obj.field_nested_dyn(
                "estimate_distribution",
                ctx,
                Some(&DistributionGroup {
                    assignees: self.estimate_assignees,
                    labels: self.estimate_labels,
                    chart: self.estimate_chart,
                }),
            )?;
        } else {
            obj.field(
                "estimate_distribution",
                &std::collections::BTreeMap::<String, String>::new(),
            )?;
        }
        obj.field_nested_dyn("distribution", ctx, Some(&self.distribution))?;
        obj.end();
        Ok(())
    }
}
