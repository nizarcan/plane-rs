// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.workspace.label.WorkspaceLabelsEndpoint`
//! (`GET /api/workspaces/{slug}/labels/`).
//!
//! Python: `apps/api/plane/app/views/workspace/label.py:17-32`. One query,
//! `WorkspaceViewerPermission`, and `@cache_response(60 * 60 * 2)` — the
//! decorator's defaults make the cache key
//! `request.get_full_path():<user id>` and store
//! `{"data": serializer.data, "status": 200}` for two hours
//! (`plane/utils/cache.py:25-51`).
//!
//! The stored value is DRF's `ReturnList` of `ReturnDict`s, UUIDs included, so
//! it is built as a `PyValue` (`plane-cache`'s pickle model) rather than as
//! JSON: a cache hit re-renders the stored value, and the harness compares the
//! Redis entry byte for byte with Django's.

use plane_cache::PyValue;
use plane_core::error::ApiError;
use plane_db::models::label::Label;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use crate::common::cache;
use crate::common::client;
use crate::labels::queries;

/// `cache_response(60 * 60 * 2)` (`plane/app/views/workspace/label.py:21`).
const CACHE_SECONDS: i64 = 2 * 60 * 60;

/// One `PyValue` per distinct string, shared by every occurrence.
///
/// CPython's pickler memoises by object identity and a DRF `serializer.data`
/// graph shares its dict keys (the serializer's class-level field names) and the
/// empty string (CPython's singleton), so Django's frame references them with
/// `BINGET` after their first occurrence. Emitting a fresh literal per key
/// produces the same decoded value but a larger frame; see the same helper in
/// `estimates/workspace.rs` for the measurement.
#[derive(Default)]
struct Interner {
    strings: std::collections::HashMap<String, PyValue>,
}

impl Interner {
    /// A dict key: DRF's field names are class-level strings, so every object
    /// in a page shares one.
    fn key(&mut self, name: &str) -> PyValue {
        if let Some(existing) = self.strings.get(name) {
            return existing.clone();
        }
        let py = PyValue::string(name.to_string());
        self.strings.insert(name.to_string(), py.clone());
        py
    }

    /// A value read from the database. CPython shares the empty string (its
    /// singleton) and every identifier-like literal (the compiler interns them),
    /// which matters because a `ChoiceField` renders a model `choices` entry by
    /// returning the class-level object — so an estimate whose `type` is
    /// `"points"` is the same string object as the `points` dict key and Django's
    /// frame references it with `BINGET`. Two equal non-identifier values from
    /// different rows are distinct objects in Python; sharing them here only
    /// makes the frame smaller, never larger.
    fn value(&mut self, text: &str) -> PyValue {
        if text.is_empty() || is_python_identifier(text) {
            return self.key(text);
        }
        PyValue::string(text.to_string())
    }
}

/// Whether CPython would intern `text` as an identifier: ASCII letters,
/// digits and underscores, not starting with a digit.
fn is_python_identifier(text: &str) -> bool {
    let mut bytes = text.bytes();
    match bytes.next() {
        Some(first) if first.is_ascii_alphabetic() || first == b'_' => {}
        _ => return false,
    }
    bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// `WorkspaceLabelsEndpoint.get` (`plane/app/views/workspace/label.py:22-30`).
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        cache::cache_response(ctx, CACHE_SECONDS, None, true, || async move {
            let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
            let member_id = ctx
                .user
                .id()
                .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

            let statement = queries::WORKSPACE_LABELS.prepare(client(ctx)?).await?;
            let rows = client(ctx)?.query(&statement, &[&member_id, &slug]).await?;
            let mut labels = Vec::with_capacity(rows.len());
            for row in &rows {
                labels.push(Label::from_row(row, 0)?);
            }

            let mut interner = Interner::default();
            Ok((
                200,
                PyValue::return_list(
                    labels
                        .iter()
                        .map(|label| label_value(label, &mut interner))
                        .collect(),
                ),
            ))
        })
        .await
    })
}

/// `LabelSerializer(label).data`, as the value `serializer.data` holds.
fn label_value(label: &Label, strings: &mut Interner) -> PyValue {
    let project_id = label.project_id.map_or(PyValue::None, PyValue::uuid);
    let parent = label.parent_id.map_or(PyValue::None, PyValue::uuid);
    // A plain dict: `ListSerializer.to_representation` calls the child's
    // `to_representation`, which returns a plain `dict`; only the *list* is a
    // `ReturnList` (`rest_framework/serializers.py:530-551,727-737`).
    PyValue::mapping(vec![
        (strings.key("parent"), parent),
        (strings.key("name"), strings.value(&label.name)),
        (strings.key("color"), strings.value(&label.color)),
        (strings.key("id"), PyValue::uuid(label.id)),
        (strings.key("project_id"), project_id),
        (
            strings.key("workspace_id"),
            PyValue::uuid(label.workspace_id),
        ),
        (strings.key("sort_order"), PyValue::float(label.sort_order)),
    ])
}
