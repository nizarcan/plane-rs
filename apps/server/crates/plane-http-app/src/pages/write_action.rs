// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `PageViewSet.lock` / `.unlock` / `.access`
//! (`plane/app/views/page/base.py:241-283`), routes 128 and 129.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | `…/projects/{project_id}/pages/{page_id}/lock/` (128) | `POST` | `lock` (`base.py:241-250`) | [`lock`] |
//! | the same route (128) | `DELETE` | `unlock` (`base.py:252-262`) | [`unlock`] |
//! | `…/projects/{project_id}/pages/{page_id}/access/` (129) | `POST` | `access` (`base.py:264-283`) | [`access`] |
//!
//! All three run behind `ProjectPagePermission` (`base.py:79`), which the
//! runtime already enforced: a non-member, an inactive member, a page of
//! another project and a private page of someone else never reach a handler
//! (`plane/app/permissions/page.py:18-143`, ported in `plane-permissions`).
//!
//! ## What each action does
//!
//! 1. `Page.objects.get(pk=…, workspace__slug=…, projects__id=…,
//!    project_pages__deleted_at__isnull=True)` — one statement
//!    ([`queries::PAGE_ACTION_LOOKUP`]); a miss is the family 404
//!    (`{"error": "The required object does not exist."}`,
//!    `plane/app/views/base.py:226-233`).
//! 2. `lock`/`unlock` flip `is_locked` and call `page.save()` — a **full** save:
//!    every column, H26's `description_stripped` and H01's `updated_by`/
//!    `updated_at`.
//! 3. `access` reads `request.data.get("access", 0)` first
//!    (`base.py:265`) — before the lookup, so a body that is not a mapping is
//!    the `AttributeError` 500 even for a missing page — then refuses a change
//!    by a non-owner (`base.py:272-277`) and saves with the body's value.
//!
//! ## The `access` argument's two reads
//!
//! The owner test compares against `request.data.get("access", page.access)`
//! (`base.py:272`) but the write uses `access`, read earlier with a default of
//! `0` (`base.py:265`). So a body **without** `access` compares equal (no
//! owner check) and then writes `0`, making the page public. The comparison is
//! Python `!=` on the raw value, so the form string `"1"` differs from the int
//! `1`; [`python_eq_int`] reproduces that. The write goes through
//! `PositiveSmallIntegerField.get_prep_value`'s `int(value)`
//! (`django/db/models/fields/__init__.py`), so `"1"`, `true` and `1.9` all
//! write `1`, a non-numeric string is a `ValueError` 500, and a negative value
//! trips the column's `CHECK (access >= 0)` — an `IntegrityError`, i.e. the
//! family 400 `{"error": "The payload is not valid"}`.

use chrono::{DateTime, NaiveDate, Utc};
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::common::{client, jobs, lookup};
use crate::pages::write_action_queries as queries;
use crate::views::write_support::{Body, body_get, integrity};
use crate::write;

/// A `pages` row as a full `save()` sees it: every column the model writes.
#[derive(Debug, Clone)]
pub(crate) struct PageWrite {
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) created_by_id: Option<Uuid>,
    pub(crate) updated_by_id: Option<Uuid>,
    pub(crate) deleted_at: Option<DateTime<Utc>>,
    pub(crate) id: Uuid,
    pub(crate) workspace_id: Uuid,
    pub(crate) name: String,
    /// `description_json` is `NOT NULL` (`plane/db/models/page.py:32`), but a
    /// body `null` must reach the column as SQL NULL to reproduce the
    /// `IntegrityError` the view turns into `{"error": "The payload is not
    /// valid"}`.
    pub(crate) description_json: Option<JsonText>,
    pub(crate) description_binary: Option<Vec<u8>>,
    pub(crate) description_html: String,
    pub(crate) description_stripped: Option<String>,
    pub(crate) owned_by_id: Uuid,
    /// `PositiveSmallIntegerField`; `None` only while writing an explicit body
    /// `null`, which the `NOT NULL` column refuses (an `IntegrityError`, i.e.
    /// the family 400 — `IntegerField.get_prep_value(None)` returns `None`).
    pub(crate) access: Option<i16>,
    pub(crate) color: String,
    pub(crate) parent_id: Option<Uuid>,
    pub(crate) archived_at: Option<NaiveDate>,
    pub(crate) is_locked: bool,
    pub(crate) view_props: JsonText,
    pub(crate) logo_props: JsonText,
    pub(crate) is_global: bool,
    pub(crate) moved_to_page: Option<Uuid>,
    pub(crate) moved_to_project: Option<Uuid>,
    pub(crate) sort_order: f64,
    pub(crate) external_id: Option<String>,
    pub(crate) external_source: Option<String>,
}

impl PageWrite {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            created_at: row.try_get(0).map_err(ViewError::from)?,
            updated_at: row.try_get(1).map_err(ViewError::from)?,
            created_by_id: row.try_get(2).map_err(ViewError::from)?,
            updated_by_id: row.try_get(3).map_err(ViewError::from)?,
            deleted_at: row.try_get(4).map_err(ViewError::from)?,
            id: row.try_get(5).map_err(ViewError::from)?,
            workspace_id: row.try_get(6).map_err(ViewError::from)?,
            name: row.try_get(7).map_err(ViewError::from)?,
            description_json: row.try_get(8).map_err(ViewError::from)?,
            description_binary: row.try_get(9).map_err(ViewError::from)?,
            description_html: row.try_get(10).map_err(ViewError::from)?,
            description_stripped: row.try_get(11).map_err(ViewError::from)?,
            owned_by_id: row.try_get(12).map_err(ViewError::from)?,
            access: row.try_get(13).map_err(ViewError::from)?,
            color: row.try_get(14).map_err(ViewError::from)?,
            parent_id: row.try_get(15).map_err(ViewError::from)?,
            archived_at: row.try_get(16).map_err(ViewError::from)?,
            is_locked: row.try_get(17).map_err(ViewError::from)?,
            view_props: row.try_get(18).map_err(ViewError::from)?,
            logo_props: row.try_get(19).map_err(ViewError::from)?,
            is_global: row.try_get(20).map_err(ViewError::from)?,
            moved_to_page: row.try_get(21).map_err(ViewError::from)?,
            moved_to_project: row.try_get(22).map_err(ViewError::from)?,
            sort_order: row.try_get(23).map_err(ViewError::from)?,
            external_id: row.try_get(24).map_err(ViewError::from)?,
            external_source: row.try_get(25).map_err(ViewError::from)?,
        })
    }

    /// `page.save()` on an existing row: H26 then H01, then one full-row
    /// `UPDATE` (`plane/db/models/page.py:71-77`,
    /// `plane/db/models/base.py:23-45`).
    pub(crate) async fn save(&mut self, client: &Client, user: Uuid) -> Result<(), ViewError> {
        // H26 recomputes `description_stripped` from the current
        // `description_html` on every save.
        self.description_stripped = hooks::page::before_save(Some(&self.description_html));
        // H01 on an update: `updated_by` becomes the request user and
        // `created_by` keeps its stored value.
        self.updated_at = write::now();
        self.updated_by_id = hooks::audit::Audit::on_update(Some(user));

        let statement = client
            .prepare_cached(queries::PAGE_FULL_SAVE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &self.created_at,
                    &self.updated_at,
                    &self.created_by_id,
                    &self.updated_by_id,
                    &self.deleted_at,
                    &self.workspace_id,
                    &self.name,
                    &self.description_json,
                    &self.description_binary,
                    &self.description_html,
                    &self.description_stripped,
                    &self.owned_by_id,
                    &self.access,
                    &self.color,
                    &self.parent_id,
                    &self.archived_at,
                    &self.is_locked,
                    &self.view_props,
                    &self.logo_props,
                    &self.is_global,
                    &self.moved_to_page,
                    &self.moved_to_project,
                    &self.sort_order,
                    &self.external_id,
                    &self.external_source,
                    &self.id,
                ],
            )
            .await
            .map_err(integrity)?;
        Ok(())
    }
}

/// One row from a lookup statement, enforcing `.get()`'s "exactly one row".
///
/// Django's `get()` fetches `LIMIT 21` and raises `MultipleObjectsReturned` for
/// more than one row; that exception is an `ObjectDoesNotExist` subclass, so it
/// renders as the same 404 (`django/core/exceptions.py`).
fn one_page(rows: Vec<tokio_postgres::Row>) -> Result<PageWrite, ViewError> {
    match rows.len() {
        1 => PageWrite::decode(&rows[0]),
        0 => Err(ViewError::ObjectDoesNotExist),
        _ => Err(ViewError::ObjectDoesNotExist),
    }
}

/// `Page.objects.get(pk=…, workspace__slug=…, projects__id=…,
/// project_pages__deleted_at__isnull=True)` (`base.py:241-246`, `:253-258`,
/// `:265-270`).
async fn load_action_page(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    page_id: Uuid,
) -> Result<PageWrite, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_ACTION_LOOKUP.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&slug, &project_id, &page_id])
        .await
        .map_err(ViewError::from)?;
    one_page(rows)
}

/// `PagesDescriptionViewSet.partial_update`'s lookup (`base.py:518-523`):
/// [`load_action_page`] plus `Q(owned_by=user) | Q(access=0)`.
pub(crate) async fn load_description_page(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    page_id: Uuid,
    user_id: Uuid,
) -> Result<PageWrite, ViewError> {
    let statement = client
        .prepare_cached(queries::PAGE_DESCRIPTION_LOOKUP.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&slug, &project_id, &page_id, &user_id])
        .await
        .map_err(ViewError::from)?;
    one_page(rows)
}

/// The URL kwargs every action needs.
struct Params {
    slug: String,
    project_id: Uuid,
    page_id: Uuid,
    user_id: Uuid,
}

fn params(ctx: &ViewCtx<'_>) -> Result<Params, ViewError> {
    Ok(Params {
        slug: ctx
            .kwarg("slug")
            .map(str::to_string)
            .ok_or(ViewError::Key)?,
        project_id: lookup::uuid_kwarg(
            "project_id",
            ctx.kwarg("project_id").ok_or(ViewError::Key)?,
        )?,
        page_id: lookup::uuid_kwarg("page_id", ctx.kwarg("page_id").ok_or(ViewError::Key)?)?,
        user_id: jobs::user_id(ctx)?,
    })
}

/// `PageViewSet.lock` (`base.py:241-250`): `is_locked = True`, full save, 204.
pub fn lock<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let mut page =
            load_action_page(client, &params.slug, params.project_id, params.page_id).await?;
        page.is_locked = true;
        page.save(client, params.user_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `PageViewSet.unlock` (`base.py:252-262`): `is_locked = False`, full save, 204.
pub fn unlock<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let mut page =
            load_action_page(client, &params.slug, params.project_id, params.page_id).await?;
        page.is_locked = false;
        page.save(client, params.user_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `PageViewSet.access` (`base.py:264-283`).
pub fn access<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;

        // `access = request.data.get("access", 0)` (`base.py:265`) — the body is
        // read before the lookup, so a non-mapping JSON body is the
        // `AttributeError` 500 even when the page does not exist.
        let data = ctx.data()?;
        let requested = body_get(data, "access")?;

        let mut page =
            load_action_page(client, &params.slug, params.project_id, params.page_id).await?;

        // `page.access != request.data.get("access", page.access) and
        // page.owned_by_id != request.user.id` (`base.py:272-273`).
        let changes = match &requested {
            // The default is `page.access` itself: never a change.
            Body::Missing => false,
            // A `QueryDict` value is a `str`, and Python never equates a `str`
            // with an `int`.
            Body::Form(_) => true,
            Body::Json(value) => !python_eq_int(page.access.unwrap_or(0), value),
        };
        if changes && page.owned_by_id != params.user_id {
            return Ok(write::error_body(
                400,
                "Access cannot be updated since this page is owned by someone else",
            ));
        }

        // `int(None)` never runs: `null` reaches the column as SQL NULL, which
        // the `NOT NULL` constraint refuses (`IntegrityError` → 400).
        page.access = match &requested {
            Body::Missing => Some(0),
            Body::Form(text) => Some(access_from_int(python_int_str(text)?)?),
            Body::Json(Json::Null) => None,
            Body::Json(value) => Some(access_from_int(python_int(value)?)?),
        };
        page.save(client, params.user_id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// Python's `int(value)` for an `int`, `bool` or `float`
/// (`django/db/models/fields/__init__.py`, `IntegerField.get_prep_value`).
///
/// `None`, a list and a dict raise `TypeError`; a non-finite float raises
/// `OverflowError`/`ValueError`. Neither is caught, so both are the family 500.
pub(crate) fn python_int(value: &Json) -> Result<i64, ViewError> {
    match value {
        Json::Int(int) => int.as_i64().ok_or_else(|| {
            let text = int.to_string();
            ViewError::Unknown(format!("ValueError: int({text}) is out of range"))
        }),
        Json::Bool(flag) => Ok(i64::from(*flag)),
        Json::Float(float) => {
            if !float.is_finite() {
                return Err(ViewError::Unknown(
                    "ValueError: cannot convert float to integer".to_owned(),
                ));
            }
            // `int(1.9)` truncates toward zero.
            Ok(float.trunc() as i64)
        }
        Json::Str(text) => text
            .as_str()
            .map_or(Err(ViewError::Validation), python_int_str),
        Json::Null => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string, a bytes-like object or a real number, not 'NoneType'"
                .to_owned(),
        )),
        Json::List(_) | Json::Dict(_) => Err(ViewError::Unknown(
            "TypeError: int() argument must be a string, a bytes-like object or a real number, not 'list'"
                .to_owned(),
        )),
    }
}

/// CPython's `int(text)` for a base-10 string: surrounding whitespace, an
/// optional sign, digits with single underscores between them.
///
/// Non-ASCII decimal digits (`int("١٢")` is also 12 in CPython) are not
/// accepted; no Plane client sends them.
pub(crate) fn python_int_str(text: &str) -> Result<i64, ViewError> {
    let trimmed = text.trim_matches(|c: char| c.is_ascii_whitespace() || c.is_whitespace());
    let invalid = || {
        ViewError::Unknown(format!(
            "ValueError: invalid literal for int() with '{text}'"
        ))
    };
    if trimmed.is_empty() {
        return Err(invalid());
    }
    let (negative, digits) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') {
        return Err(invalid());
    }
    // CPython allows single underscores between digits only.
    if digits.contains("__") {
        return Err(invalid());
    }
    let normalized = digits.replace('_', "");
    if normalized.is_empty() || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid());
    }
    let magnitude = normalized
        .parse::<i128>()
        .map_err(|_| ViewError::Unknown("OverflowError: int too large".to_owned()))?;
    let value = if negative { -magnitude } else { magnitude };
    i64::try_from(value).map_err(|_| ViewError::Unknown("OverflowError: int too large".to_owned()))
}

/// `PositiveSmallIntegerField.get_db_prep_value` for a value that already went
/// through `int()`: the `smallint` range is a `DataError` (500), a value below
/// zero violates the column's `CHECK (access >= 0)` (an `IntegrityError`, 400).
fn access_from_int(value: i64) -> Result<i16, ViewError> {
    if !(-32768..=32767).contains(&value) {
        return Err(ViewError::Unknown(
            "psycopg.errors.NumericValueOutOfRange: smallint out of range".to_owned(),
        ));
    }
    if value < 0 {
        return Err(ViewError::Integrity);
    }
    Ok(value as i16)
}

/// Python `page.access != value` for the raw body value.
fn python_eq_int(current: i16, value: &Json) -> bool {
    let current = i64::from(current);
    match value {
        Json::Int(int) => int.as_i64() == Some(current),
        // `True == 1` in Python; `bool` is an `int` subclass.
        Json::Bool(flag) => i64::from(*flag) == current,
        Json::Float(float) => *float == current as f64,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn json(text: &str) -> Json {
        pyjson::loads(text.as_bytes(), "utf-8").expect("json")
    }

    #[test]
    fn python_int_matches_the_coercions_django_accepts() {
        assert_eq!(python_int(&json("3")).expect("int"), 3);
        assert_eq!(python_int(&json("true")).expect("bool"), 1);
        assert_eq!(python_int(&json("1.9")).expect("float"), 1);
        assert_eq!(python_int(&json("-1.9")).expect("float"), -1);
        assert_eq!(python_int(&json("\" 42 \"")).expect("str"), 42);
        assert_eq!(python_int(&json("\"1_0\"")).expect("underscore"), 10);
        assert_eq!(python_int(&json("\"+7\"")).expect("sign"), 7);
        for bad in [
            "null", "[1]", "{}", "\"abc\"", "\"1__0\"", "\"_1\"", "\"1.5\"",
        ] {
            assert!(
                matches!(python_int(&json(bad)), Err(ViewError::Unknown(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn comparison_follows_python_equality() {
        assert!(python_eq_int(0, &json("0")));
        assert!(python_eq_int(1, &json("true")));
        assert!(python_eq_int(1, &json("1.0")));
        assert!(!python_eq_int(0, &json("\"0\"")));
        assert!(!python_eq_int(0, &json("null")));
        assert!(!python_eq_int(0, &json("[]")));
    }

    #[test]
    fn out_of_range_and_negative_access_values() {
        assert_eq!(access_from_int(0).expect("public"), 0);
        assert_eq!(access_from_int(1).expect("private"), 1);
        assert_eq!(access_from_int(-1), Err(ViewError::Integrity));
        assert!(matches!(access_from_int(40000), Err(ViewError::Unknown(_))));
    }
}
