// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The three version/description tasks of Phase 8 set A3:
//!
//! | task | Python | effect |
//! | --- | --- | --- |
//! | `issue_description_version_task` | `plane/bgtasks/issue_description_version_task.py:44-76` | insert or update one `IssueDescriptionVersion` |
//! | `page_transaction` | `plane/bgtasks/page_transaction_task.py:85-138` | `PageLog` rows for added `<mention-component>`/`<image-component>` tags, soft-delete the logs of removed tags |
//! | `track_page_version` | `plane/bgtasks/page_version_task.py:22-80` | **no-op, upstream bug** (see below) |
//!
//! # `track_page_version` never writes
//!
//! Both write branches read `page.description`, which `Page` does not define
//! (`plane/db/models/page.py:27-68`: the field is `description_json`). The
//! attribute access raises `AttributeError` at `page_version_task.py:55`
//! (update) and `:63` (create), the outer `except Exception` logs it and the
//! task returns. Verified against the Django oracle: a page whose
//! `description_html` changed keeps its `PageVersion` rows untouched and no
//! `soft_delete_related_objects` message is published. The handler reproduces
//! the bug (`plans/CUTOVER.md` §6: reproduce, do not fix); the trim at
//! `:72-74`, which is the publish at `:74`, is therefore unreachable. That is
//! what lets task 9 (`soft_delete_related_objects`) move to Rust — see
//! `plane_jobs::policy::PRODUCERS`.
//!
//! # `page_transaction`'s upstream details, kept
//!
//! * The parse is `BeautifulSoup(html, "html.parser").find_all(tag)`; the
//!   attribute reader is `Tag.get`, so names are lowercased and character
//!   references are decoded. [`parse_components`] is a structural port of
//!   CPython's `HTMLParser` start-tag parse (`html/parser.py:408-454`).
//! * `entity_identifier` is a `UUIDField` (`page.py:97`) and an image tag's
//!   `src` is a URL, so a page with an `<image-component src="/api/…">`
//!   raises `ValidationError` inside `bulk_create`, the whole atomic block
//!   rolls back, and **no** `PageLog` row is written. Same for a
//!   `<mention-component>` with no `entity_name` (NOT NULL). The handler
//!   validates every planned row first, which is the same all-or-nothing
//!   effect.
//! * `PageLog.objects.filter(transaction__in=deleted).delete()` has **no page
//!   filter** (`page_transaction_task.py:136`): removing a component on one
//!   page soft-deletes the log rows of every page that carries the same id.
//!   `PageLog` uses `SoftDeletionManager`, so `.delete()` is
//!   `SoftDeletionQuerySet.delete` → `UPDATE … SET deleted_at=now()`
//!   (`plane/db/mixins.py:33-38`), not a `DELETE`.
//! * The delete runs after `bulk_create`, so an invalid transaction id in the
//!   removed set leaves the inserted rows committed and skips the delete; the
//!   handler does exactly that.
//! * `bulk_create(..., ignore_conflicts=True)` is `ON CONFLICT DO NOTHING`,
//!   so a duplicate `(page, transaction)` is skipped, not an error.
//!
//! # `issue_description_version_task`'s upstream details, kept
//!
//! * `IssueDescriptionVersion.log_issue_description_version` passes
//!   `created_by_id`/`updated_by_id`, but `IssueDescriptionVersion.objects
//!   .create()` calls `BaseModel.save()` with no current user, which
//!   overwrites both with `NULL` (`plane/db/models/base.py:23-50`). The
//!   inserted row keeps `NULL` audit users; `difftest task` proves it.
//! * The update branch writes exactly five columns
//!   (`description_json`, `description_html`, `description_binary`,
//!   `description_stripped`, `last_saved_at`): `save(update_fields=…)` leaves
//!   `updated_at` where it was.
//! * `json.loads(updated_issue)` is `plane_serde::pyjson::loads`, so NaN and
//!   the CPython error classes behave as in the worker. A non-string truthy
//!   argument is `TypeError`; both failures are the outer `except`, logged
//!   and successful.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_jobs::worker::{HandlerError, JobContext, TaskRegistry};
use serde_json::Value;
use uuid::Uuid;

use crate::{TaskContext, kwargs, swallow, uuid_value};

pub const ISSUE_DESCRIPTION_VERSION: &str =
    "plane.bgtasks.issue_description_version_task.issue_description_version_task";
pub const PAGE_TRANSACTION: &str = "plane.bgtasks.page_transaction_task.page_transaction";
pub const TRACK_PAGE_VERSION: &str = "plane.bgtasks.page_version_task.track_page_version";

/// The three task names this module registers.
pub const TASKS: &[&str] = &[
    ISSUE_DESCRIPTION_VERSION,
    PAGE_TRANSACTION,
    TRACK_PAGE_VERSION,
];

/// `should_update_existing_version`'s `max_time_difference`
/// (`issue_description_version_task.py:13`).
const DESCRIPTION_VERSION_WINDOW_SECONDS: i64 = 600;

/// Register the three handlers.
pub fn register(registry: &mut TaskRegistry, context: Arc<TaskContext>) {
    let descriptions = Arc::clone(&context);
    registry.register(ISSUE_DESCRIPTION_VERSION, move |job| {
        handle_description_version(Arc::clone(&descriptions), job)
    });
    let transactions = Arc::clone(&context);
    registry.register(PAGE_TRANSACTION, move |job| {
        handle_page_transaction(Arc::clone(&transactions), job)
    });
    let versions = Arc::clone(&context);
    registry.register(TRACK_PAGE_VERSION, move |job| {
        handle_page_version(Arc::clone(&versions), job)
    });
}

fn db<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

// ---------------------------------------------------------------------------
// issue_description_version_task
// ---------------------------------------------------------------------------

/// One issue row as the task reads it
/// (`issue_description_version_task.py:49-58`).
///
/// `created_by_id`/`updated_by_id` are deliberately absent: the create path
/// passes them to `objects.create()`, but `BaseModel.save()` overwrites both
/// with `NULL` because a task has no current user (`base.py:23-50`).
struct IssueDescription {
    id: Uuid,
    workspace_id: Uuid,
    project_id: Uuid,
    description_binary: Option<Vec<u8>>,
    description_html: String,
    description_stripped: Option<String>,
    description_json: JsonText,
}

/// `except Exception: log_exception(...); return` (`:71-76`).
async fn handle_description_version(
    _context: Arc<TaskContext>,
    mut job: JobContext,
) -> Result<(), HandlerError> {
    if let Err(error) = run_description_version(&mut job.client, &job.payload).await {
        swallow(error, ISSUE_DESCRIPTION_VERSION);
    }
    Ok(())
}

async fn run_description_version(client: &mut Client, payload: &Value) -> Result<(), String> {
    let kwargs = kwargs(payload).ok_or("payload has no kwargs object")?;
    let updated_issue = kwargs.get("updated_issue");
    let current_issue = decode_updated_issue(updated_issue)?;
    let issue_id = uuid_value(kwargs.get("issue_id")).ok_or("issue_id is not a UUID")?;
    let user_id = uuid_value(kwargs.get("user_id"));
    let is_creating = py_truthy(kwargs.get("is_creating"));

    // `Issue.objects.get(id=issue_id)`: the default manager is
    // `SoftDeletionManager`, i.e. `deleted_at IS NULL` (`plane/db/mixins.py:52-55`);
    // `IssueManager` is the named `issue_objects` manager and is not used here.
    let row = client
        .query_opt(
            "SELECT id, workspace_id, project_id, \
                    description_binary, description_html, description_stripped, description_json \
             FROM issues WHERE id = $1 AND deleted_at IS NULL",
            &[&issue_id],
        )
        .await
        .map_err(db)?;
    // `except Issue.DoesNotExist: return`.
    let Some(row) = row else {
        return Ok(());
    };
    let issue = IssueDescription {
        id: row.try_get(0).map_err(db)?,
        workspace_id: row.try_get(1).map_err(db)?,
        project_id: row.try_get(2).map_err(db)?,
        description_binary: row.try_get(3).map_err(db)?,
        description_html: row.try_get(4).map_err(db)?,
        description_stripped: row.try_get(5).map_err(db)?,
        description_json: row.try_get(6).map_err(db)?,
    };

    // `if current_issue.get("description_html") == issue.description_html and
    // not is_creating: return` (`:55-56`).
    if !is_creating
        && current_issue
            .as_ref()
            .and_then(|current| current.get("description_html"))
            .and_then(|value| value.as_str())
            == Some(issue.description_html.as_str())
    {
        return Ok(());
    }

    // `with transaction.atomic():` (`:58`).
    let transaction = client.transaction().await.map_err(db)?;

    // `IssueDescriptionVersion.objects.filter(issue_id=issue_id)
    //  .order_by("-last_saved_at").first()` (`:60-62`).
    let latest = transaction
        .query_opt(
            "SELECT id, owned_by_id, last_saved_at FROM issue_description_versions \
             WHERE issue_id = $1 AND deleted_at IS NULL \
             ORDER BY last_saved_at DESC LIMIT 1",
            &[&issue.id],
        )
        .await
        .map_err(db)?;

    let now = Utc::now();
    let mut updated = false;
    if let Some(latest) = latest {
        let version_id: Uuid = latest.try_get(0).map_err(db)?;
        let owned_by_id: Uuid = latest.try_get(1).map_err(db)?;
        let last_saved_at: DateTime<Utc> = latest.try_get(2).map_err(db)?;
        // `should_update_existing_version`: same user and inside 600 s.
        let same_user = user_id == Some(owned_by_id);
        let inside = (now - last_saved_at)
            .num_microseconds()
            .is_some_and(|micros| micros <= DESCRIPTION_VERSION_WINDOW_SECONDS * 1_000_000);
        if same_user && inside {
            // `save(update_fields=[description_json, description_html,
            // description_binary, description_stripped, last_saved_at])`
            // (`:24-37`). `updated_at` is deliberately untouched.
            transaction
                .execute(
                    "UPDATE issue_description_versions \
                     SET description_json = $1, description_html = $2, \
                         description_binary = $3, description_stripped = $4, \
                         last_saved_at = $5 \
                     WHERE id = $6",
                    &[
                        &issue.description_json,
                        &issue.description_html,
                        &issue.description_binary,
                        &issue.description_stripped,
                        &now,
                        &version_id,
                    ],
                )
                .await
                .map_err(db)?;
            updated = true;
        }
    }

    if !updated {
        // `IssueDescriptionVersion.log_issue_description_version`
        // (`plane/db/models/issue.py:800-822`). `created_by_id` and
        // `updated_by_id` are the issue's, but `BaseModel.save()` overwrites
        // them with `NULL` because a task has no current user.
        let id = Uuid::new_v4();
        let created_at = Utc::now();
        transaction
            .execute(
                "INSERT INTO issue_description_versions \
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                  project_id, workspace_id, issue_id, description_binary, description_html, \
                  description_stripped, description_json, last_saved_at, owned_by_id) \
                 VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
                &[
                    &id,
                    &created_at,
                    &created_at,
                    &issue.project_id,
                    &issue.workspace_id,
                    &issue.id,
                    &issue.description_binary,
                    &issue.description_html,
                    &issue.description_stripped,
                    &issue.description_json,
                    &now,
                    &user_id,
                ],
            )
            .await
            .map_err(db)?;
    }

    transaction.commit().await.map_err(db)
}

/// `json.loads(updated_issue) if updated_issue else {}`
/// (`issue_description_version_task.py:46`).
///
/// Python's truthiness decides whether `json.loads` even runs; a truthy
/// non-string is `TypeError` inside the task's `try`. Both error classes are
/// swallowed with no database effect, so only the parsed object matters.
fn decode_updated_issue(
    value: Option<&Value>,
) -> Result<Option<plane_serde::pyjson::PyDict>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::Bool(false) => Ok(None),
        Value::Number(number) if number.as_f64() == Some(0.0) => Ok(None),
        Value::Array(items) if items.is_empty() => Ok(None),
        Value::Object(map) if map.is_empty() => Ok(None),
        Value::String(text) if text.is_empty() => Ok(None),
        Value::String(text) => {
            let parsed = plane_serde::pyjson::loads(text.as_bytes(), "utf-8")
                .map_err(|error| error.message().to_owned())?;
            parsed
                .into_dict()
                .map(Some)
                .ok_or_else(|| "the JSON value is not an object".to_owned())
        }
        other => Err(format!(
            "TypeError: the JSON object must be str, bytes or bytearray, not {}",
            json_type_name(other)
        )),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// Python truthiness of a JSON value.
fn py_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(number)) => number.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
    }
}

// ---------------------------------------------------------------------------
// track_page_version
// ---------------------------------------------------------------------------

/// `track_page_version` (`page_version_task.py:22-80`): a no-op because of the
/// missing `Page.description` attribute, after the page lookup and the JSON
/// decode.
async fn handle_page_version(
    _context: Arc<TaskContext>,
    job: JobContext,
) -> Result<(), HandlerError> {
    if let Err(error) = run_page_version(&job.client, &job.payload).await {
        swallow(error, TRACK_PAGE_VERSION);
    }
    Ok(())
}

async fn run_page_version(client: &Client, payload: &Value) -> Result<(), String> {
    let kwargs = kwargs(payload).ok_or("payload has no kwargs object")?;
    let page_id = uuid_value(kwargs.get("page_id")).ok_or("page_id is not a UUID")?;

    // `Page.objects.get(id=page_id)`; the default manager is
    // `SoftDeletionManager` (`plane/db/mixins.py:52-55`).
    let row = client
        .query_opt(
            "SELECT description_html FROM pages WHERE id = $1 AND deleted_at IS NULL",
            &[&page_id],
        )
        .await
        .map_err(db)?;
    let Some(row) = row else {
        return Ok(());
    };
    let description_html: String = row.try_get(0).map_err(db)?;

    // `current_instance = json.loads(existing_instance) if existing_instance is
    // not None else {}` (`:28`). A non-string or a JSON non-object is an
    // `AttributeError`/`TypeError` at `.get(...)`, also swallowed.
    let existing = kwargs.get("existing_instance");
    let current: Option<plane_serde::pyjson::PyDict> = match existing {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            let parsed = plane_serde::pyjson::loads(text.as_bytes(), "utf-8")
                .map_err(|error| error.message().to_owned())?;
            Some(
                parsed
                    .into_dict()
                    .ok_or_else(|| "the JSON value is not an object".to_owned())?,
            )
        }
        Some(other) => {
            return Err(format!(
                "TypeError: JSON object must be str, bytes or bytearray, not {}",
                json_type_name(other)
            ));
        }
    };

    // `if current_instance.get("description_html") != page.description_html:`
    // (`:31`).
    let unchanged = current
        .as_ref()
        .and_then(|current| current.get("description_html"))
        .and_then(|value| value.as_str())
        == Some(description_html.as_str());
    if unchanged {
        return Ok(());
    }

    // `page.description` does not exist (`plane/db/models/page.py:27-68`), so
    // both branches raise before any write: `:55` in the update branch,
    // `:63` in the create branch. The outer `except Exception` logs and
    // returns, so there is no row change and no published task.
    tracing::warn!(
        target: "plane.tasks",
        task = TRACK_PAGE_VERSION,
        page = %page_id,
        "upstream bug: 'Page' object has no attribute 'description'; \
         track_page_version writes no PageVersion row (page_version_task.py:55,63)"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// page_transaction
// ---------------------------------------------------------------------------

/// The `COMPONENT_MAP` attribute lists (`page_transaction_task.py:18-38`).
const MENTION_ATTRIBUTES: [&str; 4] = ["id", "entity_identifier", "entity_name", "entity_type"];
const IMAGE_ATTRIBUTES: [&str; 2] = ["id", "src"];

/// One parsed component tag, attribute names lowercased as `html.parser`
/// reports them.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Component {
    /// `Tag.get(name)` for every attribute in the component's
    /// `COMPONENT_MAP` entry; a missing name is absent, a value-less attribute
    /// is the empty string (BeautifulSoup's `None` → `""`,
    /// `bs4/builder/_htmlparser.py:117-120`).
    pub attributes: BTreeMap<String, String>,
}

impl Component {
    fn get(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }
}

/// `extract_all_components` (`page_transaction_task.py:41-68`) for both
/// component types.
fn extract_all_components(html: &str) -> (Vec<Component>, Vec<Component>) {
    (
        extract_components(html, "mention-component", &MENTION_ATTRIBUTES),
        extract_components(html, "image-component", &IMAGE_ATTRIBUTES),
    )
}

fn extract_components(html: &str, tag: &str, attributes: &[&str]) -> Vec<Component> {
    let mut components = Vec::new();
    scan_tags(html, tag, &mut components);
    for component in &mut components {
        component
            .attributes
            .retain(|name, _| attributes.contains(&name.as_str()));
    }
    components
}

/// `HTMLParser`'s raw-text elements (`html/parser.py:115`).
const RAW_TEXT: [&str; 6] = ["script", "style", "xmp", "iframe", "noembed", "noframes"];
/// `HTMLParser`'s RCDATA elements (`html/parser.py:116`).
const RCDATA: [&str; 2] = ["textarea", "title"];

/// Walk `html` the way CPython 3.12's `HTMLParser.feed`/`goahead` does and
/// collect every `<tag …>` start tag. `BeautifulSoup`'s builder reads
/// `handle_starttag`'s `(name, attrs)` pairs, so this is the parse the task
/// sees.
fn scan_tags(html: &str, wanted: &str, out: &mut Vec<Component>) {
    let mut index = 0usize;
    let mut cdata: Option<&str> = None;
    while index < html.len() {
        if let Some(element) = cdata {
            // `set_cdata_mode`: the content up to `</element` is data, never a
            // tag. `strip_tags` (plane-db) has the same loop.
            let Some(offset) = cdata_end(&html[index..], element) else {
                return;
            };
            index += offset;
            let Some(end) = tag_end(&html[index..]) else {
                return;
            };
            index += end + 1;
            cdata = None;
            continue;
        }
        let Some(offset) = html[index..].find('<') else {
            return;
        };
        let start = index + offset;
        index = start;
        let rest = &html[index..];
        let after = rest.as_bytes().get(1).copied().unwrap_or(0);
        if after.is_ascii_alphabetic() {
            // `parse_starttag` + `check_for_whole_start_tag`: an unterminated
            // tag is buffered and `feed()` is never closed, so nothing after
            // it exists. `locatetagend` is the greedy attribute regex, not a
            // quote-skipping scan: a stray quote can appear in an attribute
            // *name*.
            let Some(end) = locate_start_tag_end(html, index) else {
                return;
            };
            let endpos = end + 1;
            let raw = &html[index..endpos];
            if let Some((name, attributes, self_closing)) = parse_start_tag(raw) {
                if name == wanted {
                    out.push(Component { attributes });
                }
                if !self_closing {
                    if name == "plaintext" {
                        return;
                    }
                    if let Some(element) = RAW_TEXT
                        .iter()
                        .chain(RCDATA.iter())
                        .copied()
                        .find(|candidate| *candidate == name)
                    {
                        cdata = Some(element);
                    }
                }
            }
            index = endpos;
        } else if after == b'/' {
            let following = rest.as_bytes().get(2).copied().unwrap_or(0);
            if following == b'>' {
                index += 3;
            } else if following.is_ascii_alphabetic() {
                let Some(end) = tag_end(rest) else {
                    return;
                };
                index += end + 1;
            } else {
                let Some(end) = rest.find('>') else {
                    return;
                };
                index += end + 1;
            }
        } else if rest.starts_with("<!--") {
            match comment_end(rest) {
                Some(end) => index += end,
                None => return,
            }
        } else if rest.starts_with("<?") {
            let Some(end) = rest.find('>') else {
                return;
            };
            index += end + 1;
        } else if rest.starts_with("<![CDATA[") {
            let Some(end) = rest.find("]]>") else {
                return;
            };
            index += end + 3;
        } else if rest
            .get(..9)
            .is_some_and(|head| head.eq_ignore_ascii_case("<!doctype"))
        {
            let Some(end) = rest[9..].find('>') else {
                return;
            };
            index += 9 + end + 1;
        } else if rest.starts_with("<!") {
            let Some(end) = rest.find('>') else {
                return;
            };
            index += end + 1;
        } else {
            index += 1;
        }
    }
}

/// `locatetagend` + `check_for_whole_start_tag` (`html/parser.py:54-67,456-465`):
/// the index of the `>` that ends the start tag at `start`, or `None` when the
/// greedy attribute grammar cannot reach one (the tag is then buffered forever
/// and `feed()` is never closed).
fn locate_start_tag_end(html: &str, start: usize) -> Option<usize> {
    debug_assert_eq!(html.as_bytes().get(start), Some(&b'<'));
    let bytes = html.as_bytes();
    let mut cursor = start + 1;
    while cursor < bytes.len()
        && !matches!(
            bytes[cursor],
            b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/' | b'>'
        )
    {
        cursor += 1;
    }
    cursor = skip_slashes(html, cursor);
    while let Some((_, _, next)) = next_attribute(html, cursor) {
        if next == cursor {
            break;
        }
        cursor = skip_slashes(html, next);
    }
    (bytes.get(cursor) == Some(&b'>')).then_some(cursor)
}

/// `locatetagend`'s trailing `[\t\n\r\f /]*`: unlike `attrfind_tolerant` and
/// `tagfind_tolerant` it consumes every `/`, including the one before `>`.
fn skip_slashes(raw: &str, mut cursor: usize) -> usize {
    let bytes = raw.as_bytes();
    while cursor < bytes.len()
        && matches!(bytes[cursor], b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/')
    {
        cursor += 1;
    }
    cursor
}

/// `parse_starttag`'s `(tag, attrs)` result for one `<…>` span.
///
/// `None` when CPython treats the span as data instead of a tag: the text left
/// after the attributes must strip to `>` or `/>`
/// (`html/parser.py:439-442`).
fn parse_start_tag(raw: &str) -> Option<(String, BTreeMap<String, String>, bool)> {
    debug_assert!(raw.starts_with('<'));
    debug_assert!(raw.ends_with('>'));
    let bytes = raw.as_bytes();
    let mut cursor = 1usize;
    while cursor < bytes.len()
        && !matches!(
            bytes[cursor],
            b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/' | b'>'
        )
    {
        cursor += 1;
    }
    let name = raw[1..cursor].to_ascii_lowercase();
    // `tagfind_tolerant`'s trailing `(?:[\t\n\r\f ]|/(?!>))*`
    // (`html/parser.py:41`).
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\t' | b'\n' | b'\r' | 0x0c | b' ' => cursor += 1,
            b'/' if bytes.get(cursor + 1) != Some(&b'>') => cursor += 1,
            _ => break,
        }
    }
    let mut attributes = BTreeMap::new();
    while let Some((attribute, value, next)) = next_attribute(raw, cursor) {
        // `bs4`'s builder replaces an earlier duplicate with the later one
        // (`bs4/builder/_htmlparser.py:121-133`).
        attributes.insert(attribute, value);
        cursor = next;
    }
    let tail = raw[cursor..].trim();
    if tail != ">" && tail != "/>" {
        return None;
    }
    Some((name, attributes, tail == "/>"))
}

/// `attrfind_tolerant.match(rawdata, k)` (`html/parser.py:42-53`), as a
/// `(name, value, next offset)` triple. `None` means no attribute starts at
/// `cursor`.
fn next_attribute(raw: &str, mut cursor: usize) -> Option<(String, String, usize)> {
    let bytes = raw.as_bytes();
    if cursor == 0 || cursor >= bytes.len() {
        return None;
    }
    // `(?<=['"\t\n\r\f /])`.
    if !matches!(
        bytes[cursor - 1],
        b'\'' | b'"' | b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/'
    ) {
        return None;
    }
    // `[^\t\n\r\f />][^\t\n\r\f /=>]*`.
    if matches!(
        bytes[cursor],
        b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/' | b'>'
    ) {
        return None;
    }
    let name_start = cursor;
    cursor += 1;
    while cursor < bytes.len()
        && !matches!(
            bytes[cursor],
            b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/' | b'>' | b'='
        )
    {
        cursor += 1;
    }
    let name = raw[name_start..cursor].to_ascii_lowercase();
    let after_name = cursor;

    // `([\t\n\r\f ]*=[\t\n\r\f ]* (…))?`.
    while cursor < bytes.len() && matches!(bytes[cursor], b'\t' | b'\n' | b'\r' | 0x0c | b' ') {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'=') {
        return Some((name, String::new(), skip_separators(raw, after_name)));
    }
    cursor += 1;
    while cursor < bytes.len() && matches!(bytes[cursor], b'\t' | b'\n' | b'\r' | 0x0c | b' ') {
        cursor += 1;
    }
    let value = match bytes.get(cursor).copied() {
        Some(quote @ (b'\'' | b'"')) => {
            let Some(close) = raw[cursor + 1..].find(quote as char) else {
                // The quoted alternative fails and the bare alternative is
                // refused by `(?!['"])`; the optional group backtracks.
                return Some((name, String::new(), skip_separators(raw, after_name)));
            };
            let value = &raw[cursor + 1..cursor + 1 + close];
            cursor += 1 + close + 1;
            value
        }
        _ => {
            // `(?!['"])[^>\t\n\r\f ]*`, possibly empty.
            let start = cursor;
            while cursor < bytes.len()
                && !matches!(bytes[cursor], b'>' | b'\t' | b'\n' | b'\r' | 0x0c | b' ')
            {
                cursor += 1;
            }
            &raw[start..cursor]
        }
    };
    cursor = skip_separators(raw, cursor);
    // `if attrvalue: attrvalue = unescape(attrvalue)` (`:434-435`).
    let value = if value.is_empty() {
        String::new()
    } else {
        plane_db::hooks::html::unescape(value)
    };
    Some((name, value, cursor))
}

/// `(?:[\t\n\r\f ]|/(?!>))*` at `cursor`.
fn skip_separators(raw: &str, mut cursor: usize) -> usize {
    let bytes = raw.as_bytes();
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\t' | b'\n' | b'\r' | 0x0c | b' ' => cursor += 1,
            b'/' if bytes.get(cursor + 1) != Some(&b'>') => cursor += 1,
            _ => break,
        }
    }
    cursor
}

/// `locatetagend`: the byte offset of the `>` that ends a tag, skipping quoted
/// attribute values.
fn tag_end(text: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (offset, byte) in text.bytes().enumerate() {
        match quote {
            Some(active) => {
                if byte == active {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(offset),
                _ => {}
            },
        }
    }
    None
}

/// `commentclose = r'--!?>'`, falling back to `-?>` (`html/parser.py:197-223`).
fn comment_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 4;
    while index + 1 < bytes.len() {
        if bytes[index] == b'-' && bytes[index + 1] == b'-' {
            let mut cursor = index + 2;
            if bytes.get(cursor) == Some(&b'!') {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'>') {
                return Some(cursor + 1);
            }
        }
        index += 1;
    }
    let abrupt = &text[4..];
    let mut cursor = 0;
    if abrupt.starts_with('-') {
        cursor = 1;
    }
    if abrupt[cursor..].starts_with('>') {
        return Some(4 + cursor + 1);
    }
    None
}

/// `interesting = re.compile(r"</%s(?=[\t\n\r\f />])", re.I|re.A)`.
fn cdata_end(text: &str, element: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        if bytes[index] == b'<' && bytes[index + 1] == b'/' {
            let start = index + 2;
            let end = start + element.len();
            if end <= bytes.len()
                && text[start..end].eq_ignore_ascii_case(element)
                && matches!(
                    bytes.get(end),
                    Some(b'\t' | b'\n' | b'\r' | 0x0c | b' ' | b'/' | b'>')
                )
            {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// One `PageLog` row the task plans to insert.
struct PlannedLog {
    transaction: Uuid,
    entity_identifier: Option<Uuid>,
    entity_name: String,
}

/// `page_transaction` body (`page_transaction_task.py:85-138`).
async fn handle_page_transaction(
    _context: Arc<TaskContext>,
    mut job: JobContext,
) -> Result<(), HandlerError> {
    if let Err(error) = run_page_transaction(&mut job.client, &job.payload).await {
        swallow(error, PAGE_TRANSACTION);
    }
    Ok(())
}

async fn run_page_transaction(client: &mut Client, payload: &Value) -> Result<(), String> {
    let kwargs = kwargs(payload).ok_or("payload has no kwargs object")?;
    let page_id = uuid_value(kwargs.get("page_id")).ok_or("page_id is not a UUID")?;

    // `Page.objects.get(pk=page_id)`; `PageLog.objects.filter(page_id=…)
    // .exists()` (`:90-93`). Both use `SoftDeletionManager`.
    let page = client
        .query_opt(
            "SELECT workspace_id FROM pages WHERE id = $1 AND deleted_at IS NULL",
            &[&page_id],
        )
        .await
        .map_err(db)?;
    let Some(page) = page else {
        return Ok(());
    };
    let workspace_id: Uuid = page.try_get(0).map_err(db)?;
    let has_existing_logs = client
        .query_opt(
            "SELECT 1 FROM page_logs WHERE page_id = $1 AND deleted_at IS NULL LIMIT 1",
            &[&page_id],
        )
        .await
        .map_err(db)?
        .is_some();

    let (old_mentions, old_images) = components(kwargs.get("old_description_html"));
    let (new_mentions, new_images) = components(kwargs.get("new_description_html"));

    // `old_ids = {m.get("id") for m in old_entities if m.get("id")}` and the
    // same for the new side (`:114-116`); the sets hold the raw attribute
    // strings.
    let old_ids: BTreeSet<&str> = old_mentions
        .iter()
        .chain(old_images.iter())
        .filter_map(|component| component.get("id"))
        .filter(|id| !id.is_empty())
        .collect();
    let new_ids: BTreeSet<&str> = new_mentions
        .iter()
        .chain(new_images.iter())
        .filter_map(|component| component.get("id"))
        .filter(|id| !id.is_empty())
        .collect();
    let deleted: BTreeSet<&str> = old_ids.difference(&new_ids).copied().collect();

    // `for mention in new_entities: … if not mention_id or (mention_id in
    // old_ids and has_existing_logs): continue` (`:118-127`).
    let mut planned = Vec::new();
    for component in new_mentions
        .iter()
        .map(|c| (c, false))
        .chain(new_images.iter().map(|c| (c, true)))
    {
        let (component, image) = component;
        let Some(id) = component.get("id").filter(|id| !id.is_empty()) else {
            continue;
        };
        if old_ids.contains(id) && has_existing_logs {
            continue;
        }
        // `PageLog(transaction=mention_id, …)`: a UUIDField.
        let transaction = Uuid::parse_str(id).map_err(|error| {
            format!("ValidationError: transaction {id:?} is not a UUID: {error}")
        })?;
        let (entity_name, entity_identifier) = if image {
            // `COMPONENT_MAP["image-component"]` (`:30-38`).
            ("image".to_owned(), component.get("src"))
        } else {
            // `COMPONENT_MAP["mention-component"]` (`:18-28`).
            let name = component
                .get("entity_name")
                .ok_or("IntegrityError: entity_name is NULL")?;
            (name.to_owned(), component.get("entity_identifier"))
        };
        // `CharField` NOT NULL only rejects `None`; a value-less attribute is
        // the empty string and is legal. The column is `varchar(30)`.
        if entity_name.chars().count() > 30 {
            return Err(format!(
                "DataError: entity_name is longer than 30 characters ({})",
                entity_name.chars().count()
            ));
        }
        if entity_name.contains('\0') {
            return Err("DataError: entity_name contains a NUL byte".to_owned());
        }
        // `entity_identifier` is a UUIDField (`page.py:97`); an image `src`
        // URL fails validation and rolls the whole `bulk_create` back.
        let entity_identifier = match entity_identifier {
            None => None,
            Some(value) => Some(Uuid::parse_str(value).map_err(|error| {
                format!("ValidationError: entity_identifier {value:?} is not a UUID: {error}")
            })?),
        };
        planned.push(PlannedLog {
            transaction,
            entity_identifier,
            entity_name,
        });
    }

    // `if new_transactions: PageLog.objects.bulk_create(new_transactions,
    // batch_size=50, ignore_conflicts=True)` (`:133-135`). Django wraps every
    // batch in one outermost `transaction.atomic(savepoint=False)`, so a
    // failure anywhere leaves nothing behind; the validation above is the
    // same all-or-nothing decision.
    if !planned.is_empty() {
        let insert = client
            .prepare_cached(
                "INSERT INTO page_logs \
                 (id, created_at, updated_at, created_by_id, updated_by_id, deleted_at, \
                  transaction, page_id, entity_identifier, entity_name, entity_type, workspace_id) \
                 VALUES ($1, $2, $3, NULL, NULL, NULL, $4, $5, $6, $7, NULL, $8) \
                 ON CONFLICT DO NOTHING",
            )
            .await
            .map_err(db)?;
        let transaction = client.transaction().await.map_err(db)?;
        for log in &planned {
            let now = Utc::now();
            let id = Uuid::new_v4();
            transaction
                .execute(
                    &insert,
                    &[
                        &id,
                        &now,
                        &now,
                        &log.transaction,
                        &page_id,
                        &log.entity_identifier,
                        &log.entity_name,
                        &workspace_id,
                    ],
                )
                .await
                .map_err(db)?;
        }
        transaction.commit().await.map_err(db)?;
    }

    // `if deleted_transaction_ids: PageLog.objects.filter(
    // transaction__in=deleted_transaction_ids).delete()` (`:136-137`). A
    // non-UUID id in the set makes the queryset compile raise *after* the
    // inserts committed, so the delete is skipped; the filter has no page
    // filter on purpose.
    if !deleted.is_empty() {
        let mut ids = Vec::with_capacity(deleted.len());
        for id in &deleted {
            match Uuid::parse_str(id) {
                Ok(id) => ids.push(id),
                Err(error) => {
                    return Err(format!(
                        "ValidationError: transaction {id:?} is not a UUID: {error}"
                    ));
                }
            }
        }
        // `QuerySet.delete()` on a `SoftDeletionManager` model is
        // `SoftDeletionQuerySet.delete` → `self.update(deleted_at=now)`
        // (`plane/db/mixins.py:33-38`): a soft delete, one UPDATE, and
        // `updated_at` does not move. There is no reverse relation to cascade.
        client
            .execute(
                "UPDATE page_logs SET deleted_at = now() \
                 WHERE deleted_at IS NULL AND transaction = ANY($1)",
                &[&ids],
            )
            .await
            .map_err(db)?;
    }
    Ok(())
}

/// `extract_all_components(description_html)` (`page_transaction_task.py:41-68`).
///
/// `if not description_html` returns two empty lists, and any non-string
/// input raises inside the `try` (which returns the same empty lists), so only
/// a non-empty string is parsed.
fn components(value: Option<&Value>) -> (Vec<Component>, Vec<Component>) {
    match value {
        Some(Value::String(html)) if !html.is_empty() => extract_all_components(html),
        _ => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mention(attributes: &[(&str, &str)]) -> Component {
        Component {
            attributes: attributes
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn components_are_parsed_like_beautifulsoup() {
        let (mentions, images) = extract_all_components(
            "<p>x</p><mention-component id=\"11111111-1111-1111-1111-111111111111\" \
             entity_name=\"user\" entity_identifier=\"22222222-2222-2222-2222-222222222222\">\
             </mention-component><image-component id=\"33333333-3333-3333-3333-333333333333\" \
             src=\"/api/assets/x/\"/>",
        );
        assert_eq!(mentions.len(), 1);
        assert_eq!(
            mentions[0].get("entity_identifier"),
            Some("22222222-2222-2222-2222-222222222222")
        );
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].get("src"), Some("/api/assets/x/"));
    }

    #[test]
    fn attribute_values_are_lowercased_and_unescaped() {
        let (mentions, _) = extract_all_components(
            "<MENTION-COMPONENT ID=\"a&amp;b\" ENTITY_NAME=\"u&quot;x\" entity_name=\"last\">",
        );
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].get("id"), Some("a&b"));
        assert_eq!(mentions[0].get("entity_name"), Some("last"));
    }

    #[test]
    fn raw_text_and_comments_are_not_tags() {
        let (mentions, _) = extract_all_components(
            "<script><mention-component id=\"a\"></mention-component></script>\
             <!-- <mention-component id=\"b\"></mention-component> -->\
             <textarea><mention-component id=\"c\"></mention-component></textarea>\
             <p><mention-component id=\"d\"></mention-component></p>",
        );
        assert_eq!(mentions, vec![mention(&[("id", "d")])]);
    }

    #[test]
    fn unterminated_markup_stops_the_parse() {
        let (mentions, _) = extract_all_components(
            "<mention-component id=\"a\"></mention-component><mention-component id=\"b\"",
        );
        assert_eq!(mentions.len(), 1);
    }

    #[test]
    fn value_less_and_duplicate_attributes() {
        let (mentions, _) = extract_all_components(
            "<mention-component id entity_name=\"user\" entity_name=\"issue\"></mention-component>",
        );
        assert_eq!(mentions.len(), 1);
        assert_eq!(mentions[0].get("id"), Some(""));
        assert_eq!(mentions[0].get("entity_name"), Some("issue"));
    }

    #[test]
    fn slash_separated_attributes_parse_like_python() {
        let (mentions, _) = extract_all_components("<mention-component/id=\"a\">");
        assert_eq!(mentions, vec![mention(&[("id", "a")])]);
    }

    #[test]
    fn the_golden_corpus_matches_the_python_extractor() {
        // Recorded from `BeautifulSoup(html, "html.parser")` +
        // `extract_all_components` (CPython 3.12.13, bs4 in `apps/api/.venv`);
        // `tools/jobs/gen_component_corpus.py` regenerates it.
        let golden: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/page_components.json"))
                .expect("golden json");
        let cases = golden["cases"].as_array().expect("cases");
        assert!(cases.len() > 40, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let (mentions, images) = extract_all_components(input);
            assert_eq!(
                serde_json::to_value(&mentions).expect("mentions"),
                case["mentions"],
                "mentions for {input:?}"
            );
            assert_eq!(
                serde_json::to_value(&images).expect("images"),
                case["images"],
                "images for {input:?}"
            );
        }
    }
}
