# Notifications — port plan (read-only)

Area: `plane/app/views/notification/base.py` (`NotificationViewSet`,
`UnreadNotificationEndpoint`, `MarkAllReadNotificationViewSet`, `UserNotificationPreferenceEndpoint`)
and the `notifications` selectors in `deploy/route-groups.yaml:207-211`. The GETs are already Rust
(`crates/plane-http-app/src/notifications/`); this plan covers the write verbs. The group is
complete in the app surface: no `/api/v1/`, `/api/public/` or `/api/instances/` route touches
notifications.

## 1. Inventory (app routes with write methods)

Common prefix `/api/workspaces/{slug}/` except 122. "Owner" = `ROUTES.md`'s Owner cell.

| order | path | write methods | owner | set |
| --- | --- | --- | --- | --- |
| 117 | `users/notifications/{pk}/` | PATCH (DELETE is the guard's 405) | rust(GET) | A |
| 118 | `users/notifications/{pk}/read/` | POST, DELETE | django | A |
| 119 | `users/notifications/{pk}/archive/` | POST, DELETE | django | A |
| 121 | `users/notifications/mark-all-read/` | POST (bulk) | django | B |
| 122 | `/api/users/me/notification-preferences/` | PATCH | rust(GET) | B |

(`ROUTES.md:206-212`.) **Whole rows: 118, 119, 121.** GET-only and verified: 116 (`list`), 120
(`unread`), 122's GET. 117 stays partial: `destroy` is `DestroyModelMixin`'s and
`BaseViewSet.initial` refuses it (`ROUTES.md:625-626`, `routes.json` `viewset_guard`), so the flip
must register `destroy → guard_only`, as `base.rs:64-71` already does for `retrieve`, or DELETE
answers the pipeline's 501 instead of Django's 405.

No route's verbs belong to another group. 122 is a `users/me` path this group owns by selector
specificity: `deploy/route-groups.yaml:211` (4 literal segments) beats `:67` (`/api/users/**`), and
its GET already lives in `notifications/preference.rs:36-85`.

## 2. Shared pieces needed first

**a. Hooks.** None are new. `Notification` and `UserNotificationPreference` have no `save()`
override and no signal (`plane/db/models/notification.py:13-118`, `MODEL_HOOKS.md:1231-1252`), so a
full save is H01 only (`plane/db/models/base.py:23-44`): `updated_by = request.user`,
`updated_at = now`, `created_by`/`created_at` unchanged. Existing: `hooks::audit::Audit::on_update`
(`hooks/audit.rs:27-43`) and `write::now()` (`write/mod.rs:79-87`). 117/118/119/122 take
`updated_at` from `write::now()`; 121 must not write `updated_at` at all (see §4).

**b. Tasks.** None. These routes enqueue nothing; `TASKS.md:463,474` are the *producers* of
`Notification` rows and belong to the issue-activity domain. No Celery message may appear in the
scenario's diff.

**c. `write::generated`.** Add `plane.app.serializers.notification.UserNotificationPreferenceSerializer`
to `WRITE_SERIALIZERS` (`tools/inventory/serializers.py:1938-1967`): its five writable booleans are
the whole PATCH body (`SERIALIZERS.md:3134-3153`) and the generated `validate` gives DRF's
`BooleanField` messages for free. `NotificationSerializer` is **optional**: 117 synthesises a
one-key body (see §4), so only `snoozed_till` is ever validated.

**d. Response re-render.** Two shapes; both structs already exist.

* 117/118/119 answer `NotificationSerializer(instance).data` over a **plain**
  `Notification.objects.get` row (`base.py:158,170,178,186,194`), so the three declared read-only
  annotations `is_inbox_issue`/`is_intake_issue`/`is_mentioned_notification` have no attribute and
  DRF raises `SkipField`: the body is **22 keys, not 25** (`rest_framework/fields.py:426-458`;
  `read_only` ⇒ `required=False` at `fields.py:303`). Build the existing struct
  (`plane-serializers/src/generated/app_notification.rs`) with `FieldValue::absent()` for those
  three — precedent `states/write.rs:611-643`. `triggered_by_details` needs the same
  user/avatar join as `list`: lift `render_rows` (`base.rs:474-522`) and `decode_triggered_by`
  (`base.rs:549-587`) into `notifications/render.rs` as `pub(super)` one-row helpers (GET bytes
  unchanged). The `users`/`file_assets` joins carry no `deleted_at` predicate because Django's
  forward FK uses `_base_manager` (`base.rs:291-304`).
* 122 answers `UserNotificationPreferenceSerializer(instance).data`; reuse
  `notifications/preference.rs:60-83` with the patched values.
* 121 answers `{"message": "Successful"}` (`base.py:293`) — one key through
  `plane_query::json::Json`, like `unread.rs:52-56`.

**e. Reuse.** `views::write_support::{Body, body_get}` reproduces `request.data.get`, including the
`AttributeError` 500 for a non-mapping body (`write_support.rs:27-49`, already `pub(crate)` per
`PROGRESS.md:860-862`). `write::Serializer::{new,from_value}` (`serializer.rs:406-451`),
`bool_field` (`:608`), `datetime_field` (`:1019`), `fields::datetime` (`fields.rs:613`).
`queries::WORKSPACE_GUEST_EXISTS` (`queries.rs:74-90`) is the guest test 121 repeats
(`base.py:278-281`). `plane_db::models::notification::{Notification, UserNotificationPreference}`
carry `COLUMNS`/`from_row` (`models/notification.rs:190-286`, `:323-405`).

**P0: none.** Nothing above is shared by A and B; the only common edits are one `register` block
each in `notifications/mod.rs` and one entry each in `queries::ALL_QUERIES` (`queries.rs:92-97`),
resolved at merge. Both sets can start immediately.

## 3. Split

| set | routes | files it owns | size |
| --- | --- | --- | --- |
| **A — notification actions** | 117, 118, 119 | `notifications/write.rs` (`partial_update` + four actions), `notifications/write_queries.rs` (`NOTIFICATION_BY_PK`: 21 model columns + the `triggered_by`/avatar columns, `WHERE n.id=$1 AND w.slug=$2 AND n.receiver_id=$3 AND n.deleted_at IS NULL`; the single-row UPDATE), the `pub(super)` lifts in `base.rs`/`render.rs`, one `mod.rs` block (+ `destroy → guard_only`) | ~2.5 h |
| **B — bulk read + preferences** | 121, 122 | `notifications/write_bulk.rs` + query file (the ordered id scan, the id→timestamp `VALUES` update), `patch` added to `notifications/preference.rs`, `write/generated/user_notification_preference_serializer.rs` + the `WRITE_SERIALIZERS` entry, one `mod.rs` block | ~1.5 h |

A owns `scenarios/write/notifications.yaml`, B owns `scenarios/write/notification-preferences.yaml`,
so neither waits for the other. With one agent free, B folds into A as two more handlers (~4 h).

## 4. Risks

- **Order, no transactions.** 117: permission → `Notification.objects.get` (`base.py:158`) → body
  (`:160`) → validate (`:163`) → `save()` (`:164`) → render. 118/119: get (`:170,178,186,194`) →
  `read_at`/`archived_at = timezone.now()` (`:171,179,187,195`) → `save()` → render. 122: get
  (`:302,308`) → body (`:309`) → validate → `save()` → render. Nothing is atomic; a failure after
  the UPDATE leaves it committed. The body is parsed **after** the lookup, so `{not json` on a
  missing pk is 404 on 117/122 and 400 on 121 (which reads the body first, `:240`).
- **`request.data` is rebuilt, not reused.** 117 passes
  `{"snoozed_till": request.data.get("snoozed_till", None)}` (`base.py:160-161`) to DRF as a plain
  dict: the html-input branch does not apply (`rest_framework/utils/html.py:9-12`), so a form `""`
  is an invalid datetime instead of DRF's "empty → None", the last value wins for a repeated form
  key, and **every other key is ignored** — `read_at`, `archived_at` and `deleted_at` cannot be
  set through 117. Build it with `Serializer::from_value` (`serializer.rs:428-451`), not `new`.
- **Full save vs bulk update.** `serializer.save()` is `ModelSerializer.update` → `instance.save()`
  (`rest_framework/serializers.py:1064`), i.e. every column rewritten from the pre-image plus H01's
  `updated_by` and `auto_now`'s `updated_at`. Writing only the changed column + those two is
  identical on the row; only Django's clobber-under-concurrency differs. 121 is the opposite:
  `bulk_update(…, ["read_at"], batch_size=100)` writes `read_at` **only** (`django/db/models/query.py:907-924`),
  no `updated_at`, no `updated_by`, no hooks, and opens `transaction.atomic(savepoint=False)`
  (`:922`) *after* the scan. Stamping `updated_at` there is the easiest way to fail the DB diff.
- **`read_at` per row.** The loop calls `timezone.now()` once per notification (`base.py:289-290`)
  in `snoozed_till ASC, created_at DESC` order (`:247`), then batches by 100. Emit one `write::now()`
  per id in that order (a single value would pass the harness but is visible to a client comparing
  two rows).
- **121's filters.** `snoozed`/`archived` are `.get(..., False)` tested for Python **truthiness**
  (`base.py:251,257`): JSON `false` takes the `else`, the form string `"false"` takes the `if`. The
  `if` branch is `snoozed_till < now OR snoozed_till IS NOT NULL` (`:252`), so a future snooze
  matches both. `type` is `==` against `"watching"`/`"assigned"`/`"created"` (`:263,270,277`) — no
  comma split, and list's `"subscribed"` filters nothing. `type=created` for a guest replaces the
  queryset with `.none()` (`:278-281`). Unlike `list` (`:66,101-104`) 121 has **no**
  `entity_name="issue"` and **no** `mentioned` exclusion, so it marks comment/project/cycle/module
  rows too.
- **Manager predicates.** `Notification.objects` filters `deleted_at` (`mixins.py:56-58`) → a
  soft-deleted row is 404; `workspace__slug` is an unfiltered join (`base.rs:280-285`);
  `receiver=request.user` scopes every action. `UserNotificationPreference.objects.get(user=…)`:
  0 rows → 404, ≥2 → `MultipleObjectsReturned`, which is not `ObjectDoesNotExist` and reaches the
  generic 500 (`views/base.py:235-239`). The fixture's `member` has two rows
  (`tools/fixtures/build.py:2904-2922`), so 122 PATCH as member is a 500; only admin (one row) and
  the bot (none) exercise the other cardinalities (`scenarios/read/notifications-extra.yaml`).
- **Permissions.** `allow_permission(WORKSPACE)` is a method decorator (`permissions/base.py:19-103`,
  WORKSPACE at `:59-66`), so it runs after `IsAuthenticated` and before the lookup: a non-member
  gets 403 for any pk, including a missing one, and `role__in=[20,15,5]` + `is_active` +
  `deleted_at IS NULL` make an inactive member 403. 122 has no role check at all. The runtime
  already enforces this from `routes.json` `method_permissions`; no handler work.
- **OPTIONS.** 121 has no `serializer_class`, so DRF's `determine_actions` raises and OPTIONS is a
  500 (`routes.json` `options_metadata.error` = `AssertionError …`). The runtime replays that.
- **Contract coverage.** Only 122 PATCH: `apps/api/plane/tests/contract/app/test_user_owned_fk_scope_app.py:47-57`
  (the `user` FK is read-only, `comment` flips). **None** for 117 PATCH, 118, 119 or 121, so the
  differential scenario is the only evidence; run
  `tools/contract/run.sh --assume-rust 117,118,119,121,122 --methods POST,PATCH,DELETE`.
- **Scenario fixture.** `plane_fix_tpl` has 11 notifications and the preference rows
  (`build.py:2847-2922`). Add, by `fixture_sql` on the clone: a `triggered_by_id IS NULL` row (null
  nested object), a soft-deleted row (404), a row in `ws2` for the same receiver (slug scoping), a
  **past** `snoozed_till` (every seeded one is NULL or +3 days, `:2858-2871`, so 121's `if` branch
  has nothing to mark), and an `IssueSubscriber` for `member` on `issue_alpha`: all notifications
  point at `alpha:000` (`:2872`), which `member` created (`:1293`) and is assigned to
  (`:1249-1250`), so `type=created`/`assigned` match but `type=watching` matches nothing today
  (subscribers are seeded for `alpha:010-012` only, `:1832-1838`). Also cover the form
  `snoozed=false` body, a non-mapping body (`body: "[]"` → 500), `{not json` on a missing pk (404)
  vs on 121 (400), and a PATCH carrying `read_at`/`archived_at`/`deleted_at` (ignored). Add the new
  query catalogs to `tests/schema_check.rs::catalogs()`.
