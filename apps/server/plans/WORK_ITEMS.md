# Work items — port plan (read-only)

Area: `plane/app/views/issue/**` and `plane/app/views/workspace/draft.py`. The GETs are
already Rust (`src/issues/list_*`, `src/issues/sub_*`); this plan covers the write verbs.
Cycles, modules, intake, pages, labels and the v1/public surfaces are excluded.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Django" = the write verbs
are still Django today (`ROUTES.md`); "whole" = no Rust method on the row.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 64 | `issues/` | POST | Django (GET Rust) | A |
| 67 | `issues/{pk}/` | PATCH, DELETE (PUT 405) | Django | A |
| 71 | `bulk-delete-issues/` | DELETE | whole | A |
| 72 | `bulk-archive-issues/` | POST | whole | A |
| 92 | `issues/{pk}/archive/` | POST, DELETE | Django (GET Rust) | A |
| 96 | `issue-dates/` | POST | whole | A |
| 81 | `issues/{issue_id}/comments/` | POST | Django | B |
| 82 | `issues/{issue_id}/comments/{pk}/` | PATCH, DELETE (PUT 405) | Django | B |
| 86 | `issues/{issue_id}/reactions/` | POST | Django | B |
| 87 | `issues/{issue_id}/reactions/{reaction_code}/` | DELETE | whole | B |
| 88 | `comments/{comment_id}/reactions/` | POST | Django | B |
| 89 | `comments/{comment_id}/reactions/{reaction_code}/` | DELETE | whole | B |
| 73 | `issues/{issue_id}/sub-issues/` | POST | Django (GET Rust) | C |
| 74 | `issues/{issue_id}/issue-links/` | POST | Django | C |
| 75 | `issues/{issue_id}/issue-links/{pk}/` | PATCH, DELETE | Django | C |
| 83 | `issues/{issue_id}/issue-subscribers/` | POST | Django | C |
| 84 | `issues/{issue_id}/issue-subscribers/{subscriber_id}/` | DELETE | whole | C |
| 85 | `issues/{issue_id}/subscribe/` | POST, DELETE | Django (GET Rust) | C |
| 93 | `issues/{issue_id}/issue-relation/` | POST | Django | C |
| 94 | `issues/{issue_id}/remove-relation/` | POST | whole | C |
| 76 | `issues/{issue_id}/issue-attachments/` | POST, DELETE | Django (GET Rust) | D |
| 77 | `issues/{issue_id}/issue-attachments/{pk}/` | POST, DELETE | Django | D |
| 78 | `/api/assets/v2/.../issues/{issue_id}/attachments/` | POST, PATCH, DELETE | Django (GET Rust) | D |
| 79 | `/api/assets/v2/.../issues/{issue_id}/attachments/{pk}/` | POST, PATCH, DELETE | Django | D |
| 215 | `/api/workspaces/{slug}/draft-issues/` | POST | Django (GET Rust) | D |
| 216 | `/api/workspaces/{slug}/draft-issues/{pk}/` | PATCH, DELETE | Django | D |
| 217 | `/api/workspaces/{slug}/draft-to-issue/{draft_id}/` | POST | whole | D (after A) |

Whole rows: **71, 72, 84, 87, 89, 94, 96, 217**. 70 `bulk-create-labels` belongs to the
labels domain; 105–107 to modules; 233 is export; 55–60 intake; 33–38 cycles.

## 2. Shared pieces needed first

**a. `issue_activity` publisher** (`plane/bgtasks/issue_activities_task.py:1550`).
Signature `type, requested_data, current_instance, issue_id, actor_id, project_id, epoch,
subscriber=True, notification=False, origin=None, intake=None`. Publish exactly the kwargs
the call site passes (defaults omitted), in call-site order. App-surface shapes
(TASKS.md §4, rows 32–64): `type` a dotted string; `requested_data` a `json.dumps(...,
DjangoJSONEncoder)` string or `None`; `current_instance` a JSON string, `{}` (issue delete)
or `None`; `issue_id`/`actor_id`/`project_id` **strings** at every app site; `epoch =
int(timezone.now().timestamp())`, computed once per request where Django does; `notification`
bool; `origin = base_host(is_app=True)`; `subscriber=False` only on issue delete.
`notification_task.notifications` is never enqueued by a view — only by the worker, so no
publisher for it. Also publish: `model_activity` (issue create/patch, comment create/patch;
reuse the projects helper), `issue_description_version_task(updated_issue, issue_id,
user_id, is_creating=False)` (issue create/patch only), `crawl_work_item_link_title(id,
url)` (link create), `get_asset_object_metadata(str(asset.id))` (attachment v2 PATCH).

**b. `Issue.save()` H-numbers** (`issue.py:180-222`; MODEL_HOOKS §2.5). Create, in order:
`transaction.atomic` → `pg_advisory_xact_lock(convert_uuid_to_integer(project_id))`
(sha256 first 8 bytes, big-endian signed) → `MAX(IssueSequence.sequence)+1` (overwrites any
body `sequence_id`) → `description_stripped = plane.utils.html_processor.strip_tags(html)`
(entities decoded; **not** `hooks::html`) → `MAX(Issue.sort_order)` over `Issue.objects`
(soft-delete only: archived/triage/draft count) for project+state, `+10000` → INSERT issue →
INSERT `IssueSequence` (audit columns). Also `_ensure_default_state` (default non-triage
`State.objects`, `~is_triage`), and `_sync_completed_at` (completed group → now; on update
only when `state_id` changed). Update: description strip + full save (no sequence/sort
order). Delete: inherited soft delete → another full save plus the cascade task.

**c. m2m writes.** `IssueAssignee`/`IssueLabel`: `bulk_create` (batch 10) with the issue's
`created_by_id`/`updated_by_id`; PATCH soft-deletes the existing rows with a queryset
`.delete()` then `bulk_create(ignore_conflicts=True)`; `IntegrityError` swallowed;
`deleted_at IS NULL` is the live set. Default assignee fallback when `assignee_ids` is
absent and `project.default_assignee_id` is an active project member role ≥ 15.

**d. Description HTML/binary.** `description_html` (default `"<p></p>"`) is sanitised by
`validate_html_content` (nh3) → `{"error": ["html content is not valid"]}`; `description_binary`
is base64/10 MB/suspicious-pattern validated → `{"description_binary": ["Invalid binary data"]}`.
Comments write a `Description` row from `IssueComment.save` (atomic, then a column-only
`update_fields=["description_id"]` save) and keep it in sync on later saves.

**e. `write::generated` kinds** (`write/generated/issue_*`, `draft_*`; TODOs today):
`primary_key` (landing with stickies; note `state_id` uses `State.all_state_objects`, the
other FKs their default managers; errors `Incorrect type…` / `Invalid pk "…" - object does
not exist.`), `list` (of PKs; `Expected a list of items but got type "…".`, per-index errors,
`write_only`; then `validate()` filters assignees to active project members role ≥ 15 and
labels to the project), `date` (`DateField`, ISO), `choice` with strings (`priority`:
urgent/high/medium/low/none), `integer` (exists; `point`). Do not edit generated files — the
generator (`rs-p5-sergen`) owns them.

**f. Response render.** Create returns **201** with the hand-built `.values(...)` flat row
(the list projection + `issue_queryset_grouper` annotations + `user_timezone_converter`), not
a serializer: extract `list_list_endpoint`'s `LIST_FIELDS`/`fetch`/`decode_row`/`shift_timezone`
into a `pub(crate)` `render_issue_row(ctx, client, id)`. PATCH returns **204**; its
`current_instance` is the pre-update `IssueDetailSerializer` data — expose `list_retrieve::
IssueDetail` as a reusable builder. Delete: 204, `current_instance={}`. Satellite routes use
their generated serializers; statuses: comment/link/reaction/subscriber create 201, comment/
link patch 200, sub-issues 200 `{sub_issues, state_distribution}`, relation create 201,
archive 200 `{archived_at}`, bulk archive 200 `{archived_at}`, bulk delete 200
`{message}`, issue-dates 200 `{message}`, all deletes 204.

## 3. Split (parallel after §2)

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — issue core** | 64, 67, 71, 72, 92, 96 | `issues/write_issue.rs`, `issues/write_issue_queries.rs`, `issues/write_support.rs`, `hooks/issue.rs`, `hooks/issue_strip.rs` | ~2 h; hardest (serializer + 201 row + 3 tasks) |
| **B — comments/reactions** | 81, 82, 86, 87, 88, 89 | `issues/write_comment.rs`, `issues/write_comment_queries.rs`, `hooks/issue_comment.rs` | ~1.5 h (Description row; 5 activity shapes) |
| **C — relations/links/subscribers/sub-issues** | 73, 74, 75, 83, 84, 85, 93, 94 | `issues/write_link.rs`, `issues/write_relation.rs`, `issues/write_subscriber.rs`, `issues/write_sub_issue.rs`, `issues/write_sub_queries.rs` | ~1.5 h; link crawl + relation reverse-type mapper |
| **D — attachments/drafts** | 76–79, 215, 216, 217 | `issues/write_attachment.rs`, `issues/write_draft.rs`, `issues/write_draft_queries.rs`, `hooks/draft_issue.rs` | ~2 h; presigned uploads, draft-to-issue depends on A |

B, C and D can start as soon as the shared pieces land; D's 217 needs A's `IssueCreate`
pipeline (or waits). Each set registers its own actions in `issues/mod.rs` in one block.

## 4. Risks

- **Sequence locking/order.** The advisory lock, `MAX(IssueSequence.sequence)+1` (soft-delete
  filtered, never reused), the issue INSERT and the `IssueSequence` INSERT are one transaction;
  the key must be byte-equal to `convert_uuid_to_integer` or coexistence with Django deadlocks
  or duplicates sequences. `sort_order` uses `Issue.objects`, not `issue_objects`.
- **Transaction boundaries.** Only `Issue.save` (create) and `IssueComment.save` open
  `atomic`; the serializers/views do not. Do not wrap the whole request, and keep the
  comment's `Description` + `update_fields` second write inside its atomic block.
- **Task byte-identity.** PATCH is 204 but publishes three tasks in a fixed order
  (`issue_activity`, `model_activity`, `issue_description_version_task`); `current_instance`
  strings are DJE JSON (key order), `requested_data` for PATCH is `writable_data(...)` (drop
  read-only keys, keep request order). Bulk archive publishes activity rows *before* it may
  return 400 for a later invalid issue: the partial side effects are observable.
- **`issue_activity` renderer.** `argsrepr` is strict now; the activity call sites mix
  strings, `None`, dicts and ints, and omit trailing defaults. Each set must match its own
  call site (TASKS.md rows 32–64), not a generic shape.
- **`State.all_state_objects`** for `state_id` (includes triage/soft-deleted) versus
  `State.objects` in validate(): easy to invert.
- **Attachments.** v2 POST returns a presigned POST (`upload_data`) and creates a
  `FileAsset` with `is_uploaded=false`; v2 PATCH publishes `get_asset_object_metadata`; v1
  delete removes the S3 object and hard-deletes the row. The harness compares HTTP/DB/Redis/
  Celery but not S3 bytes; presigned URLs are normalised (D17). Use multipart for v1.
- **Drafts.** `DraftIssue.save` picks a default state, recomputes `completed_at` on every
  save, and `sort_order` on create; 217 additionally writes `CycleIssue`/`ModuleIssue` and
  two extra `issue_activity` messages (`cycle.activity.created`, `module.activity.created`)
  with different `issue_id` types.
- **Harness gaps.** No known blocker; if anything, link-title crawling and storage-metadata
  updates happen in workers (Celery-owned), so only the publish is compared. Keep
  `PLANE_RS_REQUIRE_DB_TESTS=1` schema_check catalogs per set.
