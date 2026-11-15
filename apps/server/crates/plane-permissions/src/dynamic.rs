// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `get_permissions` overrides that `routes.json` flags
//! `dynamic_permissions: true`.
//!
//! Sixteen routes resolve their permission classes from the DRF action or the
//! HTTP method instead of the class attribute. `routes.json` records the class
//! that *defines* the override in `dynamic_owners.permissions`; this module
//! turns `(defining class, method, action)` into the exact list that override
//! returns, so the runtime never has to read the Python again.
//!
//! The subtle ones:
//!
//! * `ProjectMemberListCreateAPIEndpoint` compares the method to `"GET"`
//!   literally, so `HEAD`/`OPTIONS` get `ProjectAdminPermission`, while the
//!   label and state endpoints use DRF's `SAFE_METHODS` and treat them as reads;
//!   it also imports its classes from `plane.utils.permissions`, not
//!   `plane.app.permissions` — the only override that resolves to the utils
//!   copies;
//! * `InstanceEndpoint` special-cases only `PATCH`; every other method,
//!   including `POST` and `OPTIONS`, is `AllowAny`;
//! * `IssueCommentPublicViewSet` keys on the action, not the method, so
//!   `OPTIONS`/`HEAD` (action `None`) require authentication.

use crate::PermissionClass;

const ALLOW_ANY: &[PermissionClass] = &[PermissionClass::AllowAny];
const IS_AUTHENTICATED: &[PermissionClass] = &[PermissionClass::IsAuthenticated];
const APP_PROJECT_ENTITY: &[PermissionClass] = &[PermissionClass::AppProjectEntityPermission];
const APP_PROJECT_LITE: &[PermissionClass] = &[PermissionClass::AppProjectLitePermission];
const APP_PROJECT_MEMBER: &[PermissionClass] = &[PermissionClass::AppProjectMemberPermission];
const APP_PROJECT_ADMIN: &[PermissionClass] = &[PermissionClass::AppProjectAdminPermission];
// `plane/api/views/member.py:23` imports these two from `plane.utils.permissions`,
// unlike the label and state endpoints, which use the app copies.
const UTILS_PROJECT_MEMBER: &[PermissionClass] = &[PermissionClass::UtilsProjectMemberPermission];
const UTILS_PROJECT_ADMIN: &[PermissionClass] = &[PermissionClass::UtilsProjectAdminPermission];
const INSTANCE_ADMIN: &[PermissionClass] = &[PermissionClass::InstanceAdminPermission];

/// Resolve a dynamic permission list.
///
/// `view` is either the class `routes.json` names in `dynamic_owners.permissions`
/// or a subclass that inherits the override (listed in
/// [`SUBCLASS_ALIASES`]). Returns `None` for a view with no override, and the
/// caller keeps the static `permission_classes` list.
pub fn dynamic_permission_classes(
    view: &str,
    method: &http::Method,
    action: Option<&str>,
) -> Option<&'static [PermissionClass]> {
    match canonical_view(view)? {
        "plane.app.views.issue.subscriber.IssueSubscriberViewSet" => Some(
            if matches!(
                action,
                Some("subscribe") | Some("unsubscribe") | Some("subscription_status")
            ) {
                APP_PROJECT_LITE
            } else {
                APP_PROJECT_ENTITY
            },
        ),
        "plane.space.views.issue.IssueCommentPublicViewSet" => {
            Some(if matches!(action, Some("list") | Some("retrieve")) {
                ALLOW_ANY
            } else {
                IS_AUTHENTICATED
            })
        }
        "plane.space.views.asset.EntityAssetEndpoint" => Some(if *method == http::Method::GET {
            ALLOW_ANY
        } else {
            IS_AUTHENTICATED
        }),
        "plane.license.api.views.instance.InstanceEndpoint" => {
            Some(if *method == http::Method::PATCH {
                INSTANCE_ADMIN
            } else {
                ALLOW_ANY
            })
        }
        "plane.api.views.issue.LabelListCreateAPIEndpoint" => {
            Some(if crate::is_safe_method(method) {
                APP_PROJECT_MEMBER
            } else {
                APP_PROJECT_ADMIN
            })
        }
        "plane.api.views.member.ProjectMemberListCreateAPIEndpoint" => {
            Some(if *method == http::Method::GET {
                UTILS_PROJECT_MEMBER
            } else {
                UTILS_PROJECT_ADMIN
            })
        }
        "plane.api.views.state.StateListCreateAPIEndpoint"
        | "plane.api.views.state.StateDetailAPIEndpoint" => {
            Some(if crate::is_safe_method(method) {
                APP_PROJECT_ENTITY
            } else {
                APP_PROJECT_ADMIN
            })
        }
        _ => None,
    }
}

/// Subclasses that inherit an override unchanged. `routes.json` records the
/// defining class, but a runtime that passes the route's own view class must get
/// the same answer.
pub const SUBCLASS_ALIASES: &[(&str, &str)] = &[
    (
        "plane.api.views.issue.LabelDetailAPIEndpoint",
        "plane.api.views.issue.LabelListCreateAPIEndpoint",
    ),
    (
        "plane.api.views.member.ProjectMemberDetailAPIEndpoint",
        "plane.api.views.member.ProjectMemberListCreateAPIEndpoint",
    ),
];

/// Every view with an override, as `routes.json` records it.
pub const DYNAMIC_VIEWS: &[&str] = &[
    "plane.app.views.issue.subscriber.IssueSubscriberViewSet",
    "plane.space.views.issue.IssueCommentPublicViewSet",
    "plane.space.views.asset.EntityAssetEndpoint",
    "plane.license.api.views.instance.InstanceEndpoint",
    "plane.api.views.issue.LabelListCreateAPIEndpoint",
    "plane.api.views.member.ProjectMemberListCreateAPIEndpoint",
    "plane.api.views.state.StateListCreateAPIEndpoint",
    "plane.api.views.state.StateDetailAPIEndpoint",
];

fn canonical_view(view: &str) -> Option<&str> {
    if DYNAMIC_VIEWS.contains(&view) {
        return Some(view);
    }
    SUBCLASS_ALIASES
        .iter()
        .find(|(alias, _)| *alias == view)
        .map(|(_, canonical)| *canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;

    #[test]
    fn every_recorded_view_resolves() {
        for view in DYNAMIC_VIEWS {
            assert!(
                dynamic_permission_classes(view, &Method::GET, Some("list")).is_some(),
                "{view} must resolve"
            );
        }
        assert_eq!(
            dynamic_permission_classes(
                "plane.api.views.project.ProjectListCreateAPIEndpoint",
                &Method::GET,
                None
            ),
            None
        );
    }

    #[test]
    fn subscriber_actions() {
        let view = "plane.app.views.issue.subscriber.IssueSubscriberViewSet";
        for action in ["subscribe", "unsubscribe", "subscription_status"] {
            assert_eq!(
                dynamic_permission_classes(view, &Method::POST, Some(action)),
                Some(APP_PROJECT_LITE)
            );
        }
        for action in ["list", "create", "destroy", "update"] {
            assert_eq!(
                dynamic_permission_classes(view, &Method::GET, Some(action)),
                Some(APP_PROJECT_ENTITY)
            );
        }
    }

    #[test]
    fn method_based_overrides() {
        let asset = "plane.space.views.asset.EntityAssetEndpoint";
        assert_eq!(
            dynamic_permission_classes(asset, &Method::GET, None),
            Some(ALLOW_ANY)
        );
        for method in [Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS] {
            assert_eq!(
                dynamic_permission_classes(asset, &method, None),
                Some(IS_AUTHENTICATED)
            );
        }

        let instance = "plane.license.api.views.instance.InstanceEndpoint";
        assert_eq!(
            dynamic_permission_classes(instance, &Method::PATCH, None),
            Some(INSTANCE_ADMIN)
        );
        for method in [Method::GET, Method::POST, Method::OPTIONS] {
            assert_eq!(
                dynamic_permission_classes(instance, &method, None),
                Some(ALLOW_ANY)
            );
        }

        let members = "plane.api.views.member.ProjectMemberListCreateAPIEndpoint";
        assert_eq!(
            dynamic_permission_classes(members, &Method::GET, None),
            Some(UTILS_PROJECT_MEMBER)
        );
        for method in [
            Method::HEAD,
            Method::OPTIONS,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
        ] {
            assert_eq!(
                dynamic_permission_classes(members, &method, None),
                Some(UTILS_PROJECT_ADMIN),
                "{method} must be admin-only: the Python compares to \"GET\" literally"
            );
        }

        for view in [
            "plane.api.views.issue.LabelListCreateAPIEndpoint",
            "plane.api.views.state.StateListCreateAPIEndpoint",
        ] {
            for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
                let expected = if view.contains("Label") {
                    APP_PROJECT_MEMBER
                } else {
                    APP_PROJECT_ENTITY
                };
                assert_eq!(
                    dynamic_permission_classes(view, &method, None),
                    Some(expected)
                );
            }
            for method in [Method::POST, Method::PUT, Method::PATCH, Method::DELETE] {
                assert_eq!(
                    dynamic_permission_classes(view, &method, None),
                    Some(APP_PROJECT_ADMIN)
                );
            }
        }
    }

    #[test]
    fn subclasses_inherit() {
        assert_eq!(
            dynamic_permission_classes(
                "plane.api.views.member.ProjectMemberDetailAPIEndpoint",
                &Method::GET,
                None
            ),
            Some(UTILS_PROJECT_MEMBER)
        );
        assert_eq!(
            dynamic_permission_classes(
                "plane.api.views.issue.LabelDetailAPIEndpoint",
                &Method::POST,
                None
            ),
            Some(APP_PROJECT_ADMIN)
        );
        assert_eq!(
            dynamic_permission_classes(
                "plane.api.views.state.StateDetailAPIEndpoint",
                &Method::DELETE,
                None
            ),
            Some(APP_PROJECT_ADMIN)
        );
    }
}
