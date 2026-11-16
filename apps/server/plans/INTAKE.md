# Intake — port plan (read-only)

Area: `plane/app/views/intake/base.py` (`IntakeViewSet`, `IntakeIssueViewSet`) and the `intake`
selectors in `deploy/route-groups.yaml:197-205`. The GETs are already Rust
(`crates/plane-http-app/src/intake/`); this plan covers the write verbs. v1 orders 288/289 and public
234–236 are other groups, Phase 7 (`ROUTES.md:344-345`, `:413-415`).

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/projects/{project_id}/`. "Rust" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 53 | `intakes/` | POST | rust(GET) | A |
| 54 | `intakes/{pk}/` | DELETE (PATCH is the guard's 405) | rust(GET) | A |
| 57 | `inboxes/` | POST (same action as 53) | rust(GET) | A |
| 58 | `inboxes/{pk}/` | DELETE (same action as 54) | rust(GET) | A |
| 55 | `intake-issues/` | POST | rust(GET) | B |
| 59 | `inbox-issues/` | POST (same action as 55) | rust(GET) | B |
| 56 | `intake-issues/{pk}/` | PATCH, DELETE | rust(GET) | C (PATCH), D (DELETE) |
| 60 | `inbox-issues/{pk}/` | PATCH, DELETE (same two actions as 56) | rust(GET) | C, D |

(`ROUTES.md:143-150`.) **Whole rows: none.** Five handlers cover eight paths: the view never looks
at the path, and `intake-issues`/`inbox-issues` are two patterns for one view
(`plane/app/urls/intake.py:26-35`, `:46-55`). 61/62 (`description-versions/`) and 160 (`intake-state/`, class
in `plane/app/views/state/base.py` but selected by this group, `deploy/route-groups.yaml:205`) are
GET-only and verified.

**53/57 POST never writes.** `@allow_permission` sits on `perform_create`
(`plane/app/views/intake/base.py:79-81`), not `create`, and DRF calls the hook positionally
(`rest_framework/mixins.py:17-21`), so the decorator receives the *serializer* as `request` and
raises `AttributeError: 'IntakeSerializer' object has no attribute 'user'` (verified by calling it).
`is_valid(raise_exception=True)` runs first: 400 for an invalid body, the family 500 body for a valid
one, no row, no task — and no role check either. Port that shape.

**54/58 PATCH** is the `BaseViewSet` guard's 405 (`routes.json` `viewset_guard.refused_methods`;
`plane/app/views/base.py:103-198`), the same reason `retrieve` is registered as `guard_only`
(`intake/base.rs:44-45`). A must register `partial_update → guard_only` or the row cannot flip whole.

## 2. Shared pieces needed first

**a. Hooks.** Only H16 is new: `IntakeIssue.save` floors `snoozed_till` to midnight in the
**project's** timezone when set and new/changed (`plane/db/models/intake.py:100-109`,
`plane/utils/timezone_converter.py:98-100`, `MODEL_HOOKS.md:2146`). It reads the timezone through
`Project.objects.values_list(...)`, the default manager, so a soft-deleted project is a 404 — unlike
H07. H02 is not needed as a module: "changed" is the caller's pre-image comparison, the same shape
as H10b's `state_changed` (`plane-db/src/hooks/issue.rs:398-417`). Existing: H01
(`hooks/audit.rs:27-43`), H03 (`hooks/soft_delete.rs:42-59`), H10a/H10b/H11/H12 behind
`create_issue_row`/`update_issue_row`, and H19 (`hooks/state.rs:74-88`) for the triage fallback.
There is no standalone H07 helper: `create_issue_row` re-derives `workspace_id` from the project for
`issues` only (`hooks/issue.rs:265-285`), so the `IntakeIssue`/`Intake` inserts bind the
`workspace_id` their project lookup already returned.

**b. Tasks.** No new shapes. `IssueActivityCall::view_with_intake`
(`plane-jobs/src/celery/tasks.rs:166-184`) covers all four sites (`base.py:281`, `:443`, `:465`
notification=false); `issue_description_version_task` with `is_creating` (`tasks.rs:827-846`) covers
`:294` and `:456`; soft deletes use `write::publish_soft_delete_cascade` (`write/mod.rs:123-141`).
Intake publishes **no** `model_activity` — do not copy that from the issue-create port.

**c. Nested-body validation.** `IssueCreateSerializer` is bound to `request.data["issue"]`
(`base.py:261-269`, `:414-418`) and `write::Serializer` only starts from `request.data`
(`write/serializer.rs:382-425`). Add `Serializer::from_value(ctx, &Json, partial)` reusing `new`'s
body-shape branches (`:394-423`); the generated table is complete for this serializer
(`write/generated/issue_create_serializer.rs:602`).

**d. Issue-create reuse.** `allow_triage_state=True` only swaps the state manager
(`plane/app/serializers/issue.py:134-136`), so `validate_issue` needs a flag plus a statement beside
`VALID_STATE_IN_PROJECT`, which excludes triage (`issues/write_issue_queries.rs:445-451`). Reuse
from `issues/write_issue.rs` after `pub(crate)`: `prepare_create` (`:208-233`), `write_created_issue`
(`:243-288`, H10/H11 + m2m/default assignee), `validate_issue` (`:1014-1129`),
`recheck_manager_fields` (`:1210-1242`), `field_order`, `fk_value`, `build_issue` (`:1298-1347`),
`apply_patch` (`:1349-1395`), `ensure_state_for_save` (`:1400-1406`), `strip_html` (`:1409-1415`),
`publish_issue_activity`/`activity_base` (`:1469-1516`, already `pub(crate)`). From
`issues/write_support.rs`: `request_object` (`:86`), `base_host` (`:214`), `IssueSerializerData`
(`:237-334`), `dje_bytes` (`:404`). Rows: `issue_m2m::{insert_assignees, insert_labels,
replace_assignees, replace_labels, default_assignee}` (`issue_m2m.rs:76-198`). Triage fallback:
`hooks::state::before_save` + `states::queries::{STATE_INSERT, STATE_SEQUENCE_MAX,
STATE_PROJECT_WORKSPACE}` (`states/queries.rs:181-213`).

**e. Response re-render.** `IntakeIssue` create answers **200**, not 201 (`base.py:331`), with
`IntakeIssueDetailSerializer` over `label_ids`/`assignee_ids` (`:300-329`); PATCH answers 200 from a
*differently filtered* assignee aggregate (`:479-503`). Both differ from `retrieve`'s and from each
other: create's filters `member_project__is_active` instead of `issue__issue_assignee__deleted_at`
(`:314-322` vs `:523-533`), so it can list soft-deleted through rows. Two set-local statements plus
`pub(super)` constructors for `IntakeIssueSerializer`/`IntakeIssueDetailSerializer`
(`intake/issue.rs:342-483`), with `retrieve` (`:751-858`) refactored onto them — GET bytes unchanged.

**f. `write::generated`.** Add `plane.app.serializers.intake.IntakeSerializer` (A) and
`…IntakeIssueSerializer` (C) to `WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1939`). No new
field kind: char, json, bool, pk, int-choice, datetime cover both; `validate`/`update` are manual
(`SERIALIZERS.md:210-212`), and `description_*` is the work item, not the link.

**P0** (shared by two or more sets, or needed by a set that must not wait): H16; `from_value`;
`pub(crate)`/`allow_triage` + the triage-valid statement; the two renderer constructors; and
`intake/write_queries.rs` with `INTAKE_ISSUE_FOR_ISSUE` (the full link row, C and D). B, C and D
reuse the existing `INTAKE_ONLY` (`intake/queries.rs:95-101`) for the intake id; each set keeps its
own response/re-fetch statements in its own query file. ~1.5 h.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **P0 — shared** | none | `hooks/intake_issue.rs` + `hooks/mod.rs`; `write/serializer.rs`; visibility/flag edits in `issues/write_issue*.rs`; renderer constructors in `intake/issue.rs`; `intake/write_queries.rs` | ~1.5 h |
| **A — intake CRUD** | 53, 54, 57, 58 | `intake/write_intake{,_queries}.rs`, `write/generated/intake_serializer.rs`, one register block | ~1 h |
| **B — intake-issue create** | 55, 59 | `intake/write_intake_issue{,_queries}.rs` (project + triage lookup, INSERT, create re-fetch) | ~3 h |
| **C — intake-issue PATCH** | 56, 60 | `intake/write_intake_issue_patch{,_queries}.rs` (issue pre-image, default state, UPDATE, patch re-fetch), `write/generated/intake_issue_serializer.rs`, one register block | ~3.5 h |
| **D — intake-issue DELETE** | 56, 60 | `intake/write_intake_issue_delete{,_queries}.rs` (issue-for-delete), one register block | ~1 h |

Each set adds one `register()` called from `intake/mod.rs`; the only conflicts are `pub mod`/call
lines and `WRITE_SERIALIZERS` (keep both entries). All four start after P0 and none needs another
set's unmerged code. A's validate-then-500, B's triage fallback and C's accept transition are the
three risky pieces and each lives in its own set.

## 4. Risks

- **Order, no transactions.** No `transaction.atomic` or `select_for_update` exists in
  `plane/app/views/intake/base.py`. Never wrap a handler; `create_issue_row` opens its own
  `BEGIN`/`COMMIT` (`hooks/issue.rs:174-191`) and `issue_m2m` must stay outermost
  (`issue_m2m.rs:16-31`).
- **55/59 fails after writing.** Order: name/priority (`base.py:231-242`) → `Project.get` (`:244`, 404) →
  triage fallback (`:247-257`) → `state_id` injected (`:258`) → serializer (`:261-271`) →
  `Intake.objects.filter(...).first()` and `IntakeIssue.objects.create(intake_id=intake_id.id …)`
  (`:272-279`). A project with **no intake** is `'NoneType' object has no attribute 'id'` → 500
  **after** the issue, `issue_sequences`, assignees/labels and any new triage `State` committed, and
  **before** both tasks. Zeta (no intake) pins this.
- **The 400 writes too.** An invalid `issue` is 400 *after* the triage `State.objects.create`, which
  runs H19: `sequence = MAX(non-triage)+15000` overrides the literal 65000 and `is_triage` stays
  False (`plane/db/models/state.py:65-93`, `:117-126`). Port both.
- **Body mutation.** `request.data["issue"]["state_id"]` (`base.py:258`) is what
  `json.dumps(request.data)` sends in `requested_data` (`:283`) and `updated_issue` (`:295`). A form
  body 500s earlier at `:231` (`.get` on a string). PATCH pops `skip_activity` and `issue` before the
  `intake.activity.created` dump (`:337`, `:378`, `:467`), so that message carries neither key.
- **PATCH order (`base.py:337-505`).** flags → intake lookup (missing intake is `intake_id=None` → 404 at
  `:340-346`, not 500) → role re-checks → guest 400 → issue pre-image (`:386-403`) → issue
  serializer, whose errors **win over the intake serializer's**, and a falsy `issue` skips it
  (`:385`) → intake serializer only for admin/workspace-admin (`:427`) → issue save + two publishes
  (`:435-460`) → intake save + `notification=False` (`:462-476`) → re-fetch. An intake-save failure
  leaves the issue write and its activity committed.
- **Accept transition.** `status == 1` needs a default state (`serializers/intake.py:57-71`: default
  manager, filters `workspace=` **and** `project=`), then moves the issue out of triage with a full
  `issue.save()` (`:75-91`) — H10a/H10b/H12/H01, so `updated_by`/`updated_at` change and
  `completed_at` syncs (`plane/db/models/issue.py:240-255`). No issue activity or webhook is
  published for the move. Other statuses never touch the issue.
- **Snooze.** The serializer converts a bare `"YYYY-MM-DD"` in the **project** timezone
  (`serializers/intake.py:50-53`, `timezone_converter.py:103-112`) while DRF parsed it in the
  **user's**; H16 floors again; `null` skips both; the `or attrs[...]` fallback keeps datetime
  strings.
- **Manager predicates.** `IntakeIssue.objects.get` (`base.py:341`, `:557`) hides soft-deleted links → 404,
  while `intake_issue.issue`/`.project` dereference through `_base_manager` (no `deleted_at`), so a
  soft-deleted issue still renders (`intake/queries.rs:178-184`). `Issue.objects.filter(...).first()`
  in DELETE (`:567`) hides soft-deleted issues → `.delete()` on `None` → 500. `Intake.objects.filter(...)
  .first()` in destroy (`:85`) → `None.is_default` → 500 for a wrong pk and for a deleted intake.
  Instance deletes are pk-only (`hooks/soft_delete.rs:50-53`); H07's project re-read is a no-op the
  helper may skip.
- **Permissions.** `creator=True, model=Issue` tests `Issue.objects.filter(id=kwargs["pk"],
  created_by=user)` with no workspace/project filter and returns before the role check
  (`plane/app/permissions/base.py:51-53`). PATCH/DELETE allow `ADMIN` by role plus the
  workspace-admin fallback (`:56-93`), so a MEMBER must be the issue's creator. The handler then
  re-tests `intake_issue.created_by_id` and `ProjectMember.role <= GUEST` (`:348-375`) — a permitted
  caller can still get 400 "You cannot edit intake issues". 53/57 have no role check at all.
- **Writable columns.** `IntakeIssueSerializer.created_by` is writable (`SERIALIZERS.md:2067`) and
  H01's update branch overwrites only `updated_by` (`plane/db/models/base.py:40-42`), so PATCH can
  rewrite the creator the delete/guest checks read. `IntakeSerializer` makes `created_by`,
  `updated_by` and `deleted_at` writable (`SERIALIZERS.md:2075-2089`), but H01 and the missing insert
  path make that unobservable.
- **Contract coverage.** One app test reaches this group:
  `tests/contract/app/test_intake_issue_read_only_fields_app.py:44-56` (PATCH 56: `archived_at`
  ignored, no activity row). **None** for 53, 54, 55, 56 DELETE, 57–60; the v1 tests
  (`test_intake_issue_update.py`, `test_external_api_guest_issue_visibility.py:1494`) are out of
  scope. The scenario is the only evidence — `tools/contract/run.sh --assume-rust
  53,54,55,56,57,58,59,60 --methods POST,PATCH,DELETE`.
- **Fixture.** `plane_fix_tpl` has alpha/beta intakes (both `is_default=True`), every status, a
  soft-deleted rejected link, Triage states and alpha's default assignee
  (`tools/fixtures/build.py:2040-2103`, `:753-822`). `scenarios/write/intake*.yaml` must add: a
  **non-default** intake (DELETE success; the default one is the 400), a project with an intake and
  no triage state (fallback + H19), an intake issue whose issue is triage and one that is not
  (accept move vs no-op), a non-midnight `snoozed_till` (H16), an `IntakeIssue` whose issue is
  soft-deleted (DELETE → 500), another user's link for the creator rules, alpha (guest-restricted)
  and epsilon (`guest_view_all_features=True`). Zeta gives the no-intake trio (create 500, list 404,
  retrieve 500). Capture the created issue id from the DB diff, not the response, and add every
  `write_queries` catalog to `tests/schema_check.rs::catalogs()` (`:213`).
