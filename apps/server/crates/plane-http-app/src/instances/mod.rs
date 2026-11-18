// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `instances` (`deploy/route-groups.yaml`): the instance-admin
//! API under `/api/instances/`.
//!
//! Python: `apps/api/plane/license/` — 14 routes mounted by
//! `plane/license/urls.py`. Every one of them reads the **admin** session
//! cookie (`admin-session-id`), because the session middleware tests
//! `"instances" in request.path` (`plane/authentication/middleware/session.py:22-27`);
//! a plain `session-id` cookie is anonymous here.
//!
//! This module holds the pieces the group's handlers share; the handlers land
//! with set C of `PUBLIC_INSTANCES.md`:
//!
//! * [`serializer`] — the manual `InstanceConfigurationSerializer`
//!   (`PUBLIC_INSTANCES.md` §2e) and `PATCH`'s value normalisation (§2d);
//! * [`queries`] — the configuration statements;
//! * [`mail`] — the SMTP transport §2c ports for
//!   `email-credentials-check/` (Django's `EmailMultiAlternatives` bytes,
//!   envelope and connection behaviour; set C calls it).
//!
//! Nothing is registered yet: `register_all` (`crate::register_all`) does not
//! call an `instances::register` until the first handler exists, which is why
//! every `/api/instances/` path still answers the pipeline's 501.

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use uuid::Uuid;

pub mod mail;
pub mod queries;
pub mod serializer;

/// `InstanceConfiguration.objects.bulk_update(values, ["value"], batch_size=100)`
/// (`plane/license/api/views/configuration.py:56`) — `PUBLIC_INSTANCES.md` §2d.
///
/// Only `value` is written: `bulk_update` bypasses `save()`, so there is no
/// `updated_at`, no `updated_by_id` and no model hook, and a pair whose id does
/// not exist updates nothing instead of inserting. Django batches the list at
/// 100 rows per statement; one statement for the whole set is observably the
/// same and fewer round trips.
///
/// `updates` is `(id, value)`; an empty slice performs no statement, exactly
/// like `bulk_update([])`, which returns 0 before touching the database.
///
/// The caller renders the response from the rows it already holds, with the new
/// `value` assigned, exactly as the view does (`configuration.py:51-58`: the
/// serializer at `:58` reads the queryset's populated result cache, it does not
/// re-query), so this returns only the affected-row count.
///
/// # Errors
///
/// A database failure as the family's 500.
pub async fn bulk_update_values(
    client: &Client,
    updates: &[(Uuid, String)],
) -> Result<u64, ViewError> {
    if updates.is_empty() {
        return Ok(0);
    }
    let ids: Vec<Uuid> = updates.iter().map(|(id, _)| *id).collect();
    let values: Vec<&str> = updates.iter().map(|(_, value)| value.as_str()).collect();
    let statement = queries::UPDATE_CONFIGURATION_VALUES.prepare(client).await?;
    let updated = client.execute(&statement, &[&ids, &values]).await?;
    Ok(updated)
}
