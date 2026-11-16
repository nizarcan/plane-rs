# Cycles — port plan (read-only)

Area: `plane/app/views/cycle/**`. The GETs are already Rust (`src/cycles/*`); this plan covers
every write verb of the app group. `/api/v1/`, public/space and workspace-level cycle reads are
excluded (workspace cycles is a GET).

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Django" = the write verbs are
still Django today (`ROUTES.md`); "whole" = no Rust method on the row.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 31 | `cycles/` | POST | Django (GET Rust) | A |
| 32 | `cycles/{pk}/` | PATCH, DELETE (PUT 405) | Django (GET Rust) | A |
| 33 | `cycles/{cycle_id}/cycle-issues/` | POST | Django (GET Rust) | B |
| 34 | `cycles/{cycle_id}/cycle-issues/{issue_id}/` | DELETE (GET/PUT/PATCH 405) | Django (GET Rust) | B |
| 35 | `cycles/date-check/` | POST | whole | A |
| 36 | `user-favorite-cycles/` | POST (GET 405) | Django (GET Rust) | A |
| 37 | `user-favorite-cycles/{cycle_id}/` | DELETE | whole | A |
| 38 | `cycles/{cycle_id}/transfer-issues/` | POST | whole | B |
| 39 | `cycles/{cycle_id}/user-properties/` | PATCH | Django (GET Rust) | A |
| 40 | `cycles/{cycle_id}/archive/` | POST, DELETE | Django (GET Rust) | A |
| 41 | `archived-cycles/` | POST, DELETE (both 500) | Django (GET Rust) | A |
| 42 | `archived-cycles/{pk}/` | POST, DELETE (both 500) | Django (GET Rust) | A |

41/42 pass no `cycle_id` (41 nothing, 42 `pk`), while both handlers require `cycle_id`, so every
POST/DELETE there raises `TypeError` and answers the family 500 — port as-is.

## 2. Shared pieces needed first

**Hooks (`MODEL_HOOKS.md`).** H22 `Cycle.save` — on create
`sort_order = MIN(cycle same project) - 10000` or 65535, default manager; new
`plane_db/src/hooks/cycle.rs`, same shape as H20/H21. H25 `UserFavorite.save` already exists
(`hooks::favorite::before_insert`): `MAX(sequence) + 10000`, or 65535. H03 (cycle destroy),
H04 (queryset deletes below) and H01 (every save) are shared. `CycleIssue`/`CycleUserProperties`
have no hooks.

**`CycleIssue` m2m (route 33).** Existing rows are the ones in *other* cycles
(`~Q(cycle_id=this)`); `new_issues = set(request) - set(existing)`, filtered through
`Issue.issue_objects` by workspace+project. `bulk_create(..., batch_size=10)` has **no**
`ignore_conflicts`; Django wraps all batches in `atomic(savepoint=False)`, so a duplicate
`(cycle, issue)` rolls the whole insert back (an issue already in *this* cycle is a duplicate →
family 400, upstream bug). `bulk_update(rows, ["cycle_id"], batch_size=100)` is also
`atomic(savepoint=False)` and writes only the FK (no `updated_at`/audit). Rows carry explicit
`created_by_id`/`updated_by_id` = request user (bulk bypasses H01). DELETE is
`CycleIssue.objects.filter(...).delete()` → **H04** (bare `UPDATE ... SET deleted_at=now`; no
audit/cascade, 204 even for zero rows, activity published first). Port `issue_m2m`'s insert
helper shape, without the conflict/filter semantics.

**Transfer (route 38, `plane/utils/cycle_transfer_issues.py`).** Statement order:
1. `new_cycle` lookup by slug/project/id; **no None check** → an unknown id dereferences `None`
   (`new_cycle.end_date`) → 500;
2. `old_cycle` with six annotated counts; `None` → 400 `Source cycle not found`;
3. `estimate_type` check (project has a points estimate);
4. estimate distributions (2 queries) + burndown (points) when estimates exist;
5. assignee distribution, label distribution, burndown (issues);
6. `current_cycle.progress_snapshot = {...}` and `save(update_fields=["progress_snapshot"])` —
   only that column is written (H22 does not run, `updated_at`/`updated_by` stay);
7. `CycleIssue.objects.filter(cycle, project, slug, issue not archived/draft, state group in
   backlog/unstarted/started)` in model order `-created_at`; set `cycle_id`, `bulk_update
   (["cycle_id"], batch_size=100)`;
8. `issue_activity` (cycle.activity.created) with `requested_data={"cycles_list": []}` and
   `current_instance={"updated_cycle_issues": [...], "created_cycle_issues": []}`; 200
   `{"message": "Success"}`. Same source and destination cycle is allowed.

**Tasks.** Every site uses the existing `IssueActivityCall::view` shape and the existing
`tasks::model_activity`; no new constructor is needed.

| route | tasks, in order |
| --- | --- |
| 31 | `model_activity("cycle", id, request.data, None, user, slug, origin)` |
| 32 PATCH | `model_activity("cycle", id, request.data, current_instance, …)`; `current_instance = json.dumps(CycleSerializer(cycle).data, DJE)` |
| 32 DELETE | `issue_activity` `cycle.activity.deleted` (`issue_id=str(pk)` — the cycle id, `issues` = live cycle-issue ids) → `cycle.delete()` (H03 + `soft_delete_related_objects("db","cycle")`) → favorites H04 → recent visits hard delete |
| 33 | `issue_activity` `cycle.activity.created`, `requested_data={"cycles_list": issues}` (raw body list), `current_instance={"updated_cycle_issues": [...], "created_cycle_issues": serializers.serialize("json", created_records)}` |
| 34 | `issue_activity` `cycle.activity.deleted` (`issue_id=str(issue_id)`) **before** the H04 delete |
| 38 | `issue_activity` `cycle.activity.created` as above with empty created list |

`serializers.serialize("json", created_records)` is Django's serializer format, not
`json.dumps` of the model: `[{"model": "db.cycleissue", "pk": "…", "fields": {…}}]`, model field
order, UUIDs as strings, datetimes ISO with `Z`, `ensure_ascii=True`. No helper exists in
`plane-http-app`; add one with a golden test.

**Dates/timezones.** `CycleWriteSerializer.validate` runs only when **both** dates are present
(`start > end` → `non_field_errors: ["Start date cannot exceed end date"]`), then replaces both
with `convert_to_utc`: `Project.objects.get(id=…)` per date, project-local midnight, start +1 s
(and "today" → `now` UTC), end 23:59:00; pytz DST errors → 500. Create pre-checks both-or-neither
(exactly one → 400 `Both start date and end date are either required or are to be null`, before
the serializer); a one-sided PATCH skips the conversion. `date-check` takes raw body strings,
`str(...)` + `%Y-%m-%d` (an ISO datetime → `ValueError` 500); the three-way overlap query
`exclude(pk=body.cycle_id)` answers **200** `{"error": …, "status": false}` on a hit, 200
`{"status": true}` on a miss.

**`write::generated`.** New tables: `cycle_write_serializer` (writable: `name`,
`description`, `start_date`, `end_date`, `view_props`, `sort_order`, `external_source`,
`external_id`, `progress_snapshot`, `logo_props`, `timezone` — a `ChoiceField` over
`TIMEZONE_CHOICES` (~600 strings), `version`, `deleted_at`; read-only `workspace`, `project`,
`owned_by`, `archived_at`, `created_by`, `updated_by`), and `cycle_user_properties_serializer`
(response only: the five JSON columns). `cycle_serializer` / `cycle_issue_serializer` already
exist as read serializers and are not needed as write tables (create/update render a `.values()`
row; cycle-issue create is raw). No remaining TODO kinds: Char, DateTime, JSON, Float, Integer,
string Choice and PK are landed; the custom `validate` is handler code. The `timezone` choice
list may be hand-written if the generator's table is too large.

**Response re-render.** The reads already decode/render these rows in `cycles/base.rs`
(`CycleRow`, `decode_*_row`, `render_one`). Create/update answer a different `.values()` set
(adds `version`, `created_by`, `assignee_ids`, `status`; omits `started/unstarted/backlog`),
converted with `user_timezone_converter(..., project_timezone)`, so add a
`decode_write_row`/`render_cycle_row` beside them (or re-query the list statement).
`user_properties_value` is reusable for route 39. Archive POST returns
`{"archived_at": str(cycle.archived_at)}` — Python `str(datetime)` (space, not `T`).

## 3. Split (two sets, ≤ 2 h each)

| set | routes | files it owns |
| --- | --- | --- |
| **A — cycle core** | 31, 32, 35, 36, 37, 39, 40, 41, 42 | `cycles/write.rs`, `cycles/write_queries.rs`, `hooks/cycle.rs`; one registration block in `cycles/mod.rs`; reuses `hooks::favorite`, `user_properties_value` and the read row decoders |
| **B — cycle issues + transfer** | 33, 34, 38 | `cycles/write_issue.rs`, `cycles/write_issue_queries.rs`, `cycles/write_transfer.rs`, and the new `django serializer` helper (additive under `write/`, or owned here); one registration block in `cycles/mod.rs` |

A is CRUD/archive/properties/favourites (H22, H25, H04, H03); B is the m2m + the transfer
snapshot. Both reuse the merged `issue_activity` publisher, `model_activity`, `soft_delete` and
`Audit`.

## 4. Risks

- **Statement order**: create/update re-query the `.values()` row **after** the save and before
  `model_activity`; destroy publishes activity → soft-deletes the cycle (cascade task) → H04
  favorites → hard-deletes recent visits; archive saves the cycle then H04-deletes favorites;
  transfer saves the snapshot before moving issues and publishes last.
- **Transactions**: `bulk_create`/`bulk_update` are `atomic(savepoint=False)` across batches;
  a failing batch rolls everything back. Do not wrap the whole request; the view has no outer
  atomic.
- **Set order**: `set(issues) - set(existing)` decides `new_issues` order (PYTHONHASHSEED=0) and
  that order shows up in `created_cycle_issues`; reproduce it or keep one new issue per step.
- **Upstream bugs to port**: PATCH a missing/foreign cycle → 500 (`cycle.archived_at` on `None`);
  the completed-cycle guard builds `request_data` but passes `request.data` to the serializer
  (guard ineffective apart from the presence check); 41/42 POST/DELETE 500; unknown
  `new_cycle_id` → 500; date-check rejects ISO datetimes; start/end one-sided PATCH is not
  converted; re-adding an issue already in the cycle → 400; archive POST deletes **all users'**
  favorites, destroy hard-deletes **all users'** recent visits; `progress_snapshot` save leaves
  `updated_at`/`updated_by` untouched.
- **Harness**: the create/update `status`/`is_favorite` annotations are computed at query time
  (project timezone + `timezone.now()`), but the fixture cycle dates are far from now; the
  transfer snapshot and `completion_chart` are JSON columns and must match exactly; `archive`
  POST's `str(datetime)` is not ISO. Add a `cycles` write scenario (dates both/neither/overlap,
  archive, favourites, properties, cycle-issue add/move/remove, transfer, foreign origin) and run
  `read/cycles*.yaml`. A full read sweep is only needed if the shared `cycles/base.rs` read
  renderer changes.
