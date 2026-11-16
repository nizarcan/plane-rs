// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Code generated from Django's serializer introspection.
//!
//! `apps/server/tools/inventory/serializers.py --rust` writes one module
//! per Python serializer module. Re-run it after changing `apps/api`; the
//! files are committed so the build does not need Python.
//!
//! Serializers whose `to_representation` is hand-written are **not** here;
//! they are listed as `manual` in `apps/server/SERIALIZERS.md` and get
//! hand-written modules in a later session. A generated struct references
//! a manual nested serializer through `&dyn DrfSerialize`, so a parent
//! compiles before the child's module exists.

pub mod api_asset;
pub mod api_cycle;
pub mod api_estimate;
pub mod api_intake;
pub mod api_invite;
pub mod api_issue;
pub mod api_member;
pub mod api_module;
pub mod api_project;
pub mod api_state;
pub mod api_sticky;
pub mod api_user;
pub mod api_workspace;
pub mod app_analytic;
pub mod app_api;
pub mod app_asset;
pub mod app_cycle;
pub mod app_draft;
pub mod app_estimate;
pub mod app_exporter;
pub mod app_favorite;
pub mod app_importer;
pub mod app_intake;
pub mod app_issue;
pub mod app_module;
pub mod app_notification;
pub mod app_page;
pub mod app_project;
pub mod app_state;
pub mod app_user;
pub mod app_view;
pub mod app_webhook;
pub mod app_workspace;
pub mod license_api_admin;
pub mod license_api_instance;
pub mod license_api_user;
pub mod license_api_workspace;
pub mod space_cycle;
pub mod space_intake;
pub mod space_issue;
pub mod space_module;
pub mod space_project;
pub mod space_state;
pub mod space_user;
pub mod space_workspace;
