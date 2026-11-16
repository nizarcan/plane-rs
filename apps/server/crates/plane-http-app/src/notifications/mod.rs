// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `notifications` (`deploy/route-groups.yaml`): In-app notifications and notification preferences.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/users/notifications/**`
//! * `/api/users/me/notification-preferences/`
//!
//! | Route | Registered | Python |
//! | --- | --- | --- |
//! | `GET …/users/notifications/` | `NotificationViewSet.list` | `plane/app/views/notification/base.py:48` |
//! | `GET …/users/notifications/{pk}/` | `NotificationViewSet.retrieve` (guard's 405) | `base.py:103-198` |
//! | `GET …/users/notifications/unread/` | `UnreadNotificationEndpoint.get` | `:201` |
//! | `GET /api/users/me/notification-preferences/` | `UserNotificationPreferenceEndpoint.get` | `:296` |
//!
//! The remaining verbs (`PATCH` snooze, `POST`/`DELETE` read/archive,
//! `POST` mark-all-read) are Phase 5 and stay on Django.

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod preference;
pub mod queries;
pub mod render;
pub mod unread;

/// Register this group's handlers, in route order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    unread::register(registry)?;
    preference::register(registry)?;
    Ok(())
}
