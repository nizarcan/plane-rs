// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `webhooks` (`deploy/route-groups.yaml`): Workspace webhooks and webhook delivery logs.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/webhooks/**`
//! * `/api/workspaces/*/webhook-logs/**`
//!
//! One file per Python module under `plane/app/views/`:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`base`] | `webhook/base.py` — `WebhookEndpoint`, `WebhookLogsEndpoint`, `WebhookSecretRegenerateEndpoint` |
//!
//! The three `GET`s are ported, and set A's writes (`POST` on `webhooks/`,
//! the `pk` route's TypeError 500 and `POST …/webhooks/{pk}/regenerate/`).
//! `PATCH`/`DELETE` stay on Django (unregistered → 501) until set B registers
//! them.
//!
//! The shared write pieces live beside them:
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`validate`] | `WebhookSerializer`'s URL field chain and `_validate_webhook_url` (`serializers/webhook.py:25`, `:27-55`) |
//! | [`render`] | the 17-key create/regen body and the ten-key GET/PATCH body (`serializers/webhook.py:57-67`) |
//! | [`write`] | `WebhookEndpoint.post` and `WebhookSecretRegenerateEndpoint.post` (`views/webhook/base.py:21-39`, `:115-121`) |
//! | [`write_queries`] | `WEBHOOK_INSERT`, `WEBHOOK_UPDATE`, `WEBHOOK_BY_PK_FULL` |

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod queries;
pub mod render;
pub mod validate;
pub mod write;
pub mod write_queries;

/// Register this group's handlers, in the order they were ported.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)
}
