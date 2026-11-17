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
//! | `PATCH …/users/notifications/{pk}/` | `NotificationViewSet.partial_update` | `base.py:156` |
//! | `DELETE …/users/notifications/{pk}/` | `NotificationViewSet.destroy` (guard's 405) | `base.py:103-198` |
//! | `POST`/`DELETE …/{pk}/read/` | `mark_read` / `mark_unread` | `base.py:168,176` |
//! | `POST`/`DELETE …/{pk}/archive/` | `archive` / `unarchive` | `base.py:184,192` |
//! | `GET …/users/notifications/unread/` | `UnreadNotificationEndpoint.get` | `:201` |
//! | `POST …/users/notifications/mark-all-read/` | `MarkAllReadNotificationViewSet.create` | `:237` |
//! | `GET`/`PATCH /api/users/me/notification-preferences/` | `UserNotificationPreferenceEndpoint` | `:296` |
//!
//! The whole group is on Rust; `ROUTES.md` rows 116–122.

use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod preference;
pub mod queries;
pub mod render;
pub mod unread;
pub mod write;
pub mod write_bulk;
pub mod write_bulk_queries;
pub mod write_queries;

/// Register this group's handlers, in route order.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    base::register(registry)?;
    unread::register(registry)?;
    preference::register(registry)?;
    write::register(registry)?;
    write_bulk::register(registry)?;
    Ok(())
}
