// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two `WebhookSerializer` output shapes.
//!
//! `WebhookSerializer` is a `DynamicBaseSerializer` with `fields = "__all__"`
//! and a `to_representation` that pops `secret_key` unless the context carries
//! `show_secret_key` (`plane/app/serializers/webhook.py:57-67`). The views
//! combine that with two different `fields=` tuples, so there are two bodies:
//!
//! | View | Context | Keys |
//! | --- | --- | --- |
//! | `WebhookEndpoint.post`, `WebhookSecretRegenerateEndpoint.post` | `show_secret_key=True`, no `fields=` | 17, [`WebhookFull`] |
//! | `WebhookEndpoint.get` (both branches), `WebhookEndpoint.patch` | no `show_secret_key`, a fixed ten-name tuple | 10, [`WebhookTen`] |
//!
//! Both lists are the serializer's **declaration order** filtered by the
//! `fields=` allowlist, which is what `DynamicBaseSerializer._filter_fields`
//! produces (it only restricts the output; validation still sees every field).
//! The declaration order is `SERIALIZERS.md`'s table for
//! `plane.app.serializers.webhook.WebhookSerializer`: `id`, `url`,
//! `created_at`, `updated_at`, `deleted_at`, `is_active`, `secret_key`,
//! `project`, `issue`, `module`, `cycle`, `issue_comment`, `is_internal`,
//! `version`, `created_by`, `updated_by`, `workspace`.
//!
//! Note that the ten-name tuple lists `cycle` before `module` while the
//! declaration order is the other way round; `fields=` filters, it does not
//! reorder, so `module` still precedes `cycle` in the body (the GET scenario
//! pins this).

use chrono::{DateTime, Utc};
use plane_db::models::Webhook;
use plane_serializers::{DrfError, DrfSerialize, DrfUuid, DrfWriter, RenderCtx};
use uuid::Uuid;

use plane_drf::error::ViewError;

/// The ten keys `WebhookEndpoint.get` and `WebhookEndpoint.patch` answer
/// (`plane/app/views/webhook/base.py:45-57`, `:64-75`, `:89-101`).
///
/// [`Self::decode`] reads the read query's ten columns;
/// [`Self::from_model`] adapts a saved `plane_db::models::Webhook` for PATCH,
/// which renders the in-memory instance (`serializer.data`, not a re-read).
#[derive(Debug, Clone)]
pub(super) struct WebhookTen {
    pub id: Uuid,
    pub url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
    pub project: bool,
    pub issue: bool,
    pub module: bool,
    pub cycle: bool,
    pub issue_comment: bool,
}

impl WebhookTen {
    /// Columns 0–9 of [`super::queries::WEBHOOKS_BY_WORKSPACE`] /
    /// [`super::queries::WEBHOOK_BY_PK`].
    pub(super) fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            url: row.try_get(1).map_err(ViewError::from)?,
            created_at: row.try_get(2).map_err(ViewError::from)?,
            updated_at: row.try_get(3).map_err(ViewError::from)?,
            is_active: row.try_get(4).map_err(ViewError::from)?,
            project: row.try_get(5).map_err(ViewError::from)?,
            issue: row.try_get(6).map_err(ViewError::from)?,
            module: row.try_get(7).map_err(ViewError::from)?,
            cycle: row.try_get(8).map_err(ViewError::from)?,
            issue_comment: row.try_get(9).map_err(ViewError::from)?,
        })
    }

    /// The PATCH shape: the row as the save left it
    /// (`WebhookEndpoint.patch` renders `serializer.data` from the in-memory
    /// instance, never a re-read).
    pub(super) fn from_model(model: &Webhook) -> Self {
        Self {
            id: model.id,
            url: model.url.clone(),
            created_at: model.created_at,
            updated_at: model.updated_at,
            is_active: model.is_active,
            project: model.project,
            issue: model.issue,
            module: model.module,
            cycle: model.cycle,
            issue_comment: model.issue_comment,
        }
    }
}

impl DrfSerialize for WebhookTen {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("url", ctx, &self.url)?;
        obj.field_dyn(
            "created_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.created_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &plane_serializers::fields::drf_datetime(self.updated_at, ctx.tz()),
        )?;
        obj.field_dyn("is_active", ctx, &self.is_active)?;
        obj.field_dyn("project", ctx, &self.project)?;
        obj.field_dyn("issue", ctx, &self.issue)?;
        obj.field_dyn("module", ctx, &self.module)?;
        obj.field_dyn("cycle", ctx, &self.cycle)?;
        obj.field_dyn("issue_comment", ctx, &self.issue_comment)?;
        obj.end();
        Ok(())
    }
}

/// The 17 keys a create/regen answers with `show_secret_key=True`
/// (`plane/app/views/webhook/base.py:31`, `:120-121`).
///
/// `secret_key` is included only here; every other shape is
/// [`WebhookTen`]. `created_by`/`updated_by` are `null` for a fixture row with
/// no audit user, and `deleted_at` is `null` on anything a route returns.
#[derive(Debug, Clone, Copy)]
// No route calls this yet: set A's create/regenerate answer with it.
#[allow(
    dead_code,
    reason = "used by the create/regenerate handlers in set A; the test below pins it"
)]
pub(super) struct WebhookFull<'a> {
    pub webhook: &'a Webhook,
}

impl DrfSerialize for WebhookFull<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let webhook = self.webhook;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(webhook.id))?;
        obj.field_dyn("url", ctx, &webhook.url)?;
        obj.field_dyn(
            "created_at",
            ctx,
            &plane_serializers::fields::drf_datetime(webhook.created_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &plane_serializers::fields::drf_datetime(webhook.updated_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "deleted_at",
            ctx,
            &plane_serializers::fields::drf_datetime_opt(webhook.deleted_at, ctx.tz()),
        )?;
        obj.field_dyn("is_active", ctx, &webhook.is_active)?;
        obj.field_dyn("secret_key", ctx, &webhook.secret_key)?;
        obj.field_dyn("project", ctx, &webhook.project)?;
        obj.field_dyn("issue", ctx, &webhook.issue)?;
        obj.field_dyn("module", ctx, &webhook.module)?;
        obj.field_dyn("cycle", ctx, &webhook.cycle)?;
        obj.field_dyn("issue_comment", ctx, &webhook.issue_comment)?;
        obj.field_dyn("is_internal", ctx, &webhook.is_internal)?;
        obj.field_dyn("version", ctx, &webhook.version)?;
        obj.field_dyn("created_by", ctx, &webhook.created_by_id)?;
        obj.field_dyn("updated_by", ctx, &webhook.updated_by_id)?;
        obj.field_dyn("workspace", ctx, &DrfUuid(webhook.workspace_id))?;
        obj.end();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serializers::render;

    fn model() -> Webhook {
        Webhook {
            created_at: "2026-01-01T00:00:01Z".parse().expect("datetime"),
            updated_at: "2026-01-01T00:00:02.000123Z".parse().expect("datetime"),
            created_by_id: Some(
                "d44ae570-3de4-4413-af08-f686bf5b7235"
                    .parse()
                    .expect("uuid"),
            ),
            updated_by_id: None,
            deleted_at: None,
            id: "f8cfd225-6f45-442e-a86f-ec8317215958"
                .parse()
                .expect("uuid"),
            workspace_id: "a91c3d80-5ed0-40f3-92a8-a28c015f4388"
                .parse()
                .expect("uuid"),
            url: "https://hooks.example.test/active".to_string(),
            is_active: true,
            secret_key: "plane_wh_0123456789abcdef0123456789abcdef".to_string(),
            project: true,
            issue: true,
            module: false,
            cycle: true,
            issue_comment: false,
            is_internal: false,
            version: "v1".to_string(),
        }
    }

    fn body(value: &impl DrfSerialize) -> String {
        let bytes = render(value, &RenderCtx::full(&chrono_tz::UTC)).expect("render");
        String::from_utf8(bytes).expect("utf8")
    }

    /// The key lists `WebhookSerializer` produces, in order. The ten-key body
    /// is the one the read scenario compares byte for byte today.
    #[test]
    fn key_lists_match_the_serializer() {
        let model = model();
        assert_eq!(
            body(&WebhookFull { webhook: &model }),
            concat!(
                r#"{"id":"f8cfd225-6f45-442e-a86f-ec8317215958","#,
                r#""url":"https://hooks.example.test/active","#,
                r#""created_at":"2026-01-01T00:00:01Z","#,
                r#""updated_at":"2026-01-01T00:00:02.000123Z","#,
                r#""deleted_at":null,"#,
                r#""is_active":true,"#,
                r#""secret_key":"plane_wh_0123456789abcdef0123456789abcdef","#,
                r#""project":true,"#,
                r#""issue":true,"#,
                r#""module":false,"#,
                r#""cycle":true,"#,
                r#""issue_comment":false,"#,
                r#""is_internal":false,"#,
                r#""version":"v1","#,
                r#""created_by":"d44ae570-3de4-4413-af08-f686bf5b7235","#,
                r#""updated_by":null,"#,
                r#""workspace":"a91c3d80-5ed0-40f3-92a8-a28c015f4388"}"#,
            )
        );
        assert_eq!(
            body(&WebhookTen::from_model(&model)),
            concat!(
                r#"{"id":"f8cfd225-6f45-442e-a86f-ec8317215958","#,
                r#""url":"https://hooks.example.test/active","#,
                r#""created_at":"2026-01-01T00:00:01Z","#,
                r#""updated_at":"2026-01-01T00:00:02.000123Z","#,
                r#""is_active":true,"#,
                r#""project":true,"#,
                r#""issue":true,"#,
                r#""module":false,"#,
                r#""cycle":true,"#,
                r#""issue_comment":false}"#,
            )
        );
    }
}
