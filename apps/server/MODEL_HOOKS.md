# Model hooks, managers, audit columns and delete behaviour

Reference for porting every write path of `apps/api` to Rust. Django's ORM gives model
behaviour an automatic home: `save()` overrides run on every `Model.save()`, managers
filter every query, `delete()` cascades in Python, and middleware fills audit columns.
Rust has none of that. **Every INSERT/UPDATE/DELETE written by `plane-db` must call the
hooks listed here explicitly, in the order given.**

The Python is the specification. When this document and `apps/api` disagree, the Python
wins and this document gets fixed.

Scope of this document (Phase 0 deliverable required by `RUST_REWRITE.md`):

- all 96 concrete models in `apps/api/plane/db/models/` (92) and
  `apps/api/plane/license/models/` (4), including the `integration/` subpackage;
- every abstract base and mixin that adds behaviour (`plane/db/mixins.py`,
  `plane/db/models/base.py`);
- every custom manager/queryset;
- every signal receiver;
- every implicit write path (`bulk_create`, `bulk_update`, `QuerySet.update`, raw SQL)
  that bypasses the model hooks;
- the two delete jobs (`soft_delete_related_objects`, `hard_delete`).

Not in scope: `django.contrib.*` and `django_celery_beat` models. They exist in the same
database (`sessions`, `auth_*`, `django_celery_beat_*`) but are not Plane models; the
`Session` table is covered because Plane subclasses it.

Conventions used below:

- `file:line` always refers to the current checkout (`rust-rewrite`, upstream
  `290383d7f9`).
- "full save" means `Model.save()` with no `update_fields`: every column is written,
  `auto_now` timestamps are refreshed, `BaseModel.save()` re-runs the audit logic, and the
  model's own `save()` hook runs.
- "column-only update" means a `QuerySet.update()`/`bulk_create`/`bulk_update`: **no
  model `save()`, no `delete()`, no audit columns, no signals.** `bulk_create` still
  evaluates `auto_now`/`auto_now_add` and PK defaults; `bulk_update` rewrites only the
  named columns with the in-memory values (see §4.1).

---

## Contents

1. [Base classes, mixins, managers](#1-base-classes-mixins-managers)
2. [Per-model reference](#2-per-model-reference)
3. [Signals](#3-signals)
4. [Other implicit write behaviour](#4-other-implicit-write-behaviour)
5. [Hard delete and FK cascades](#5-hard-delete-and-fk-cascades)
6. [Hook checklist](#6-hook-checklist)
- [Appendix A — completeness grep](#appendix-a--completeness-grep)
- [Appendix B — counts](#appendix-b--counts)
- [Appendix C — write paths and the hooks they must call](#appendix-c--write-paths-and-the-hooks-they-must-call)

---

## 1. Base classes, mixins, managers

### 1.1 Inheritance map

```
django.db.models.Model
├── TimeAuditModel            plane/db/mixins.py:16
├── UserAuditModel            plane/db/mixins.py:26
├── SoftDeleteModel           plane/db/mixins.py:61   (+ SoftDeletionManager :56, SoftDeletionQuerySet :48)
└── AuditModel                plane/db/mixins.py:85   = TimeAuditModel + UserAuditModel + SoftDeleteModel
    ├── BaseModel             plane/db/models/base.py:17  (+ UUID id + save() audit hook)
    │   ├── ProjectBaseModel  plane/db/models/project.py:230  (+ project, workspace FKs + save())
    │   │   └── (Issue*, Cycle*, Module*, Estimate*, State, Intake*, Importer, Github*, Slack*)
    │   └── WorkspaceBaseModel plane/db/models/workspace.py:185 (+ workspace, project FKs + save())
    │       └── (Label, IssueView, UserFavorite, DeployBoard, DraftIssue*, Description*, UserRecentVisit)
    ├── Integration           plane/db/models/integration/base.py:16   (own UUID id, NO BaseModel)
    └── ProjectIdentifier     plane/db/models/project.py:314           (implicit BigAutoField id, NO BaseModel)

ChangeTrackerMixin           plane/db/mixins.py:92  (mixed into Issue, IssueComment, Project, IntakeIssue)
```

Everything that inherits `AuditModel` (directly or through `BaseModel`,
`ProjectBaseModel`, `WorkspaceBaseModel`) is soft-deletable. The four exceptions are
listed in §1.11.

### 1.2 `TimeAuditModel` — `plane/db/mixins.py:16`

| field | type | null | default | behaviour |
| --- | --- | --- | --- | --- |
| `created_at` | `DateTimeField` | no | — | `auto_now_add=True` |
| `updated_at` | `DateTimeField` | no | — | `auto_now=True` |

`auto_now_add` sets the value on INSERT only. `auto_now` sets it on every full save (and
on a column-only save only if `updated_at` is itself in `update_fields`). Both are set by
the **ORM in Python**, never by a database default; `apps/api/plane/db/migrations/0001_initial.py`
declares `models.DateTimeField(auto_now_add=True)` / `auto_now=True` with no `db_default`.

Django calls `timezone.now()` separately for each field, so on INSERT `created_at` and
`updated_at` can differ by microseconds. Both are timezone-aware UTC (`USE_TZ=True`,
`TIME_ZONE="UTC"`, `plane/settings/common.py:318-319`); column type is
`timestamp with time zone`.

### 1.3 `UserAuditModel` — `plane/db/mixins.py:26`

| field | type | null | on_delete |
| --- | --- | --- | --- |
| `created_by` | FK `db.User` | yes | `SET_NULL`, `related_name="%(class)s_created_by"` |
| `updated_by` | FK `db.User` | yes | `SET_NULL`, `related_name="%(class)s_updated_by"` |

No save behaviour of its own. The values are written by `BaseModel.save()` (§1.6), and
because `SET_NULL` is only emulated by Django, deleting a `users` row nulls these columns
(§5).

### 1.4 Soft delete — `plane/db/mixins.py:48-82`

```python
class SoftDeletionQuerySet(models.QuerySet):
    def delete(self, soft=True):
        if soft:
            return self.update(deleted_at=timezone.now())   # one UPDATE, no hooks
        return super().delete()                             # real DELETE, Django cascade

class SoftDeletionManager(models.Manager):
    def get_queryset(self):
        return SoftDeletionQuerySet(self.model, using=self._db).filter(deleted_at__isnull=True)

class SoftDeleteModel(models.Model):
    deleted_at = models.DateTimeField(null=True, blank=True)
    objects = SoftDeletionManager()      # deleted_at IS NULL
    all_objects = models.Manager()       # everything, including soft-deleted

    def delete(self, using=None, soft=True, *args, **kwargs):
        if soft:
            self.deleted_at = timezone.now()
            self.save(using=using)                      # FULL SAVE -> all save hooks run
            soft_delete_related_objects.delay(
                self._meta.app_label, self._meta.model_name, self.pk, using=using
            )
        else:
            return super().delete(using=using, *args, **kwargs)
```

Precise semantics:

**`instance.delete()` (soft, the default)**

1. Sets `deleted_at` in memory only (no `updated_at` change yet).
2. Calls `self.save(using=using)` — a **full save**, so the model's own `save()` hook
   runs (e.g. `Issue.save` recomputes `description_stripped`, `Page.save` recomputes
   `description_stripped`, `IssueComment.save` may update its `Description`), `updated_at`
   is refreshed, `BaseModel.save` sets `updated_by` to the current request user, and
   **every column is rewritten**. `created_by`/`updated_by` become `NULL` when there is no
   current request user (Celery task, management command) — see §1.9.
3. Enqueues `soft_delete_related_objects.delay(app_label, model_name, pk, using)`.
   This is **not** wrapped in `transaction.on_commit`, so the worker can observe the row
   before the surrounding request transaction commits. The task is not idempotency-guarded
   beyond re-reading the row.
4. Returns `None`. (Django's hard delete returns `(count, {label: count})`.)

**`instance.delete(soft=False)`** — `super().delete()`: Django's `Collector`. Child rows
are deleted (CASCADE) or nulled (SET_NULL) in Python, `DO_NOTHING` children are left
alone, `pre_delete`/`post_delete` fire (no receivers), and the FK constraints are plain
(§5).

**`Model.objects.filter(...).delete()`** — the queryset is a `SoftDeletionQuerySet`
because `objects` is a `SoftDeletionManager`, so this is the **bulk soft delete**: a single
`UPDATE ... SET deleted_at = now()` over the matching rows. It does **not** call the model
`delete()`, does **not** run save hooks, does **not** touch `updated_at`/`updated_by`, does
**not** recurse, and does **not** enqueue `soft_delete_related_objects`. Returns the
number of rows updated (an `int`, not a tuple). Callers that rely on the return value get
an int on the soft path and a tuple on the hard path.

**`Model.all_objects.filter(...).delete()`** — `all_objects` is a plain `Manager`, so its
queryset is a plain `QuerySet` and this is a real DELETE with Django's Python-side
cascade. Used by `hard_delete`.

**`Model.objects.filter(...).delete(soft=False)`** — same as above but real DELETE.

**`soft_delete_related_objects` — `plane/bgtasks/deletion_task.py:17-105`**

The Celery task that emulates cascade for soft deletes. It is *not* the same cascade as
Django's hard delete:

1. Loads the instance with `all_objects.get(pk=...)` (so an already-soft-deleted row still
   resolves); returns silently if it is gone.
2. Iterates **reverse relations only**: `f.one_to_many or f.one_to_one`, `f.auto_created`,
   `not f.concrete`. Forward FKs are never followed.
3. Skips relations whose `on_delete.__name__ == "DO_NOTHING"` (so `issue_activities.issue`
   and `issue_activities.issue_comment` are *not* soft-deleted when an issue or comment is
   deleted).
4. `SET_NULL`: for a `OneToOneRel`, loads the object and
   `save(update_fields=[field])`; for a one-to-many, `queryset.update(fk=None)` (column-only
   update, no hooks, no `updated_at`).
5. Everything else (CASCADE, and anything not SET_NULL/DO_NOTHING): obtains the related
   manager through the **`objects`** manager explicitly
   (`getattr(instance, related_name)(manager="objects").all()`), skips rows without a
   `deleted_at` attribute, and for each row whose `deleted_at` is NULL: sets
   `deleted_at = now()`, calls `save()` (full save → hooks, audit columns cleared outside a
   request), then recurses **in-process** (a direct call to the task function, not
   `.delay()`).
6. Finally sets `deleted_at` on the instance itself if it is still NULL and saves it.
7. Exceptions per relation are swallowed with `print(...)` and the loop continues.
8. `using` is passed to the recursive call but **not** to `save()`/`update()`; with one
   database this is irrelevant.

Consequences to port:

- Soft delete is **shallow at the query level but deep via the task**: `project.delete()`
  soft-deletes the project and the task soft-deletes every soft-deletable child
  (issues, cycles, modules, states, labels, members, …), recursively.
- It **does not delete child rows in tables without `deleted_at`** (there are none under a
  soft-deletable parent in `db`/`license`).
- It **does not null `created_by`/`updated_by`** on the parent in a request, but does null
  them for every child it saves from Celery (no current user).
- It is **not ordered/deduplicated**: a child reachable through two paths is visited twice
  but the second visit sees `deleted_at` set and stops.
- `restore_related_objects` (`deletion_task.py:109-110`) is a `pass` stub and is not
  scheduled.

**Where the two managers are used**

| manager | filter | used by |
| --- | --- | --- |
| `objects` (all soft models) | `deleted_at IS NULL` | nearly every read; `SoftDeletionQuerySet` |
| `all_objects` (all soft models) | none | `hard_delete`, cleanup jobs, restore-style reads, `soft_delete_related_objects` |
| `Issue.issue_objects` | `deleted_at IS NULL` **and** `NOT triage` **and** `archived_at IS NULL` **and** `project.archived_at IS NULL` **and** `NOT is_draft` | issue list/detail endpoints; **default manager of `Issue`**, so reverse FK accessors to Issue use it |
| `Issue.objects` | `deleted_at IS NULL` only | `Issue.save`'s `sort_order` aggregate; places that must see triage/archived/draft issues |
| `State.objects` | `deleted_at IS NULL` and `group != 'triage'` | default state lookup, sequence aggregate |
| `State.triage_objects` | `deleted_at IS NULL` and `group == 'triage'` | intake/triage endpoints |
| `State.all_state_objects` | none | equivalent to `all_objects` |
| `Session.objects` | Django `BaseSessionManager` (deletes expired sessions on `save()`) | session backend |
| `User.objects` | Django `UserManager` | auth |

`Issue._meta.default_manager` is `issue_objects`, not `objects`
(`apps/api` introspection; `issue_objects` is declared first on the class). This matters
for any reverse relation whose related model is `Issue` (`State.state_issue`,
`Project.project_issue`, …): Django builds the related manager from
`related_model._default_manager.__class__`, so those reverse accessors apply the
`IssueManager` filters (they hide triage, archived, archived-project and draft issues).
`soft_delete_related_objects` deliberately overrides this with `manager="objects"`.

### 1.5 `AuditModel` — `plane/db/mixins.py:85`

Empty abstract class combining `TimeAuditModel`, `UserAuditModel`, `SoftDeleteModel`. Used
directly by `Integration` and `ProjectIdentifier` (which therefore have soft delete and
audit *columns*, but **no audit hook** because they do not inherit `BaseModel`).

### 1.6 `BaseModel` and the audit hook — `plane/db/models/base.py:17-44`

Adds:

| field | type | null | default | notes |
| --- | --- | --- | --- | --- |
| `id` | `UUIDField` | no | `uuid.uuid4` | `primary_key=True`, `unique=True`, `db_index=True`, `editable=False` |

`save()` signature: `save(self, *args, created_by_id=None, disable_auto_set_user=False, **kwargs)`.
Both extra arguments are keyword-only.

Exact algorithm (`base.py:23-44`):

```
if not disable_auto_set_user:
    if created_by_id:                       # truthy UUID
        self.created_by_id = created_by_id  # updated_by untouched
    else:
        user = crum.get_current_user()      # request.user, or None outside a request
        if user is None or user.is_anonymous:
            self.created_by = None
            self.updated_by = None
        elif self._state.adding:
            self.created_by = user
            self.updated_by = None
        else:
            self.updated_by = user          # created_by untouched
super().save(*args, **kwargs)               # django.db.models.Model.save
```

Resulting column values:

| situation | `created_by` | `updated_by` |
| --- | --- | --- |
| INSERT inside an authenticated request | request user | `NULL` (explicitly) |
| INSERT with an anonymous request | `NULL` | `NULL` |
| INSERT outside a request (Celery, command, test) | `NULL` | `NULL` |
| UPDATE inside an authenticated request | unchanged | request user |
| UPDATE with an anonymous request | **set to `NULL`** | **set to `NULL`** |
| UPDATE outside a request | **set to `NULL`** | **set to `NULL`** |
| any save with `created_by_id=X` | `X` | unchanged (usually `NULL`) |
| any save with `disable_auto_set_user=True` | unchanged | unchanged |

Two traps the Rust port must reproduce (or consciously deviate from, recorded in the
ledger):

1. **A save with no request user clears both audit columns.** `soft_delete_related_objects`
   runs in a Celery worker, so every child it soft-deletes loses its `created_by`/
   `updated_by`. Any management command or task that calls `.save()` on an existing row
   does the same. (`update_fields=[...]` limits the damage to the listed columns; the code
   still assigns the attributes, but they are not written.)
2. **`update_fields` wins.** If `update_fields` does not contain `created_by`/`updated_by`,
   the null/assignment is computed in memory but not written. `updated_at` is only written
   when it is itself in `update_fields`; Django does *not* add `auto_now` fields
   automatically.

`disable_auto_set_user=True` is used by the workspace-seed task together with
`created_by_id=bot_user.id` (`plane/bgtasks/workspace_seed_task.py:113,206,239,293,376,387,436,472`).

### 1.7 `ChangeTrackerMixin` — `plane/db/mixins.py:92-221`

Not a model. Mixed in **before** the model base:

- `Issue(ChangeTrackerMixin, ProjectBaseModel)` — `TRACKED_FIELDS = ["state_id"]`
- `IssueComment(ChangeTrackerMixin, ProjectBaseModel)` — `TRACKED_FIELDS = ["comment_stripped", "comment_json", "comment_html"]`
- `Project(ChangeTrackerMixin, BaseModel)` — `TRACKED_FIELDS = ["timezone"]`
- `IntakeIssue(ChangeTrackerMixin, ProjectBaseModel)` — `TRACKED_FIELDS = ["snoozed_till"]`

Behaviour:

- `__init__` captures `_original_values` (`mixins.py:126-129`). With `TRACKED_FIELDS`, it
  stores `getattr(self, name)` for each listed name (e.g. `"state_id"` → the UUID);
  without it, it stores every concrete field by `attname`. Deferred fields are skipped.
- `has_changed(name)` (`:151-164`) compares the stored value with the current attribute.
- `changed_fields` (`:166-181`) is the list of differing tracked names.
- `save()` (`:196-210`) stores `self._changes_on_save = self.changed_fields` **before**
  `super().save()`, then resets the tracked values. `IssueComment.save` reads
  `_changes_on_save` after its first `super().save()` to decide which `Description`
  columns to rewrite.
- Deferred fields loaded with `.only()`/`.defer()` are not tracked. `Project.save` has an
  explicit fallback that re-reads `timezone` from the database when it was deferred.

Rust: the equivalent is to load the "before" values in the same query that feeds the
update (or use `RETURNING`), and pass a `changed` set into the hook.

### 1.8 Managers and querysets — exact filters

```python
# plane/db/mixins.py:48-58
SoftDeletionQuerySet:  delete(soft=True)  -> UPDATE deleted_at = now()
SoftDeletionManager:   get_queryset()     -> SoftDeletionQuerySet(...).filter(deleted_at__isnull=True)

# plane/db/models/issue.py:92-101
IssueManager(SoftDeletionManager).get_queryset():
    super().get_queryset()
      .exclude(state__group='triage')
      .exclude(archived_at__isnull=False)
      .exclude(project__archived_at__isnull=False)
      .exclude(is_draft=True)

# plane/db/models/state.py:65-76
StateManager(SoftDeletionManager):        super().get_queryset().exclude(group='triage')
TriageStateManager(SoftDeletionManager):  super().get_queryset().filter(group='triage')
```

SQL notes for the `Issue` filter (verified by compiling the queryset without a database):

- `exclude(state__group='triage')` becomes
  `LEFT OUTER JOIN states ... AND NOT (states.group = 'triage' AND states.group IS NOT NULL)`.
  Because the join is outer, rows with `state_id IS NULL` **are kept**.
- `exclude(archived_at__isnull=False)` is `NOT (issues.archived_at IS NOT NULL)`.
- `exclude(project__archived_at__isnull=False)` adds `INNER JOIN projects` (project is
  non-null) and `NOT (projects.archived_at IS NOT NULL)`.
- `exclude(is_draft=True)` is `NOT (issues.is_draft)`.
- The manager is used for reads and for reverse accessors; it is **not** what `Issue.save`
  uses for `sort_order` (that is `Issue.objects`, the plain soft-deletion manager).

### 1.9 How `created_by`/`updated_by` get set (django-crum)

- `crum.CurrentRequestUserMiddleware` is in `MIDDLEWARE` at
  `plane/settings/common.py:131`, after
  `django.contrib.auth.middleware.AuthenticationMiddleware` (with
  `XFrameOptionsMiddleware` between) and before `GZipMiddleware`.
  crum version installed: 0.7.9.
- `__call__`/`process_request` stores the `HttpRequest` in a **thread-local**
  (`crum/__init__.py:92-104`). `process_response`/`process_exception` clear it.
- `get_current_user()` (`crum/__init__.py:38-73`) asks the registered getters;
  `crum/signals.py:23-31` returns `getattr(current_request, "user", False)` at priority
  `-10`, and a thread-local override at priority `10`. With no request, both return
  `False`, so `get_current_user()` returns **`None`**. With an unauthenticated request it
  returns `AnonymousUser` (`is_anonymous == True`).
- DRF's `Request.user` setter writes the authenticated user back onto the underlying
  Django `HttpRequest` (`rest_framework/request.py:239-250`), which is what crum reads.
  So for `/api/v1/` API-key requests, by the time a view saves, crum sees the token's user
  (permissions/handlers have already forced DRF authentication).
- Django's `AuthenticationMiddleware` sets `request.user` from the session before crum
  runs; for `/api/` session requests the user is therefore available.
- There is **no** user context in Celery workers, management commands, `plane-beat`, or
  tests that do not go through a request.

Rust has no thread-local request user. Every `plane-db` write function must take the
acting user as an explicit argument (`Option<UserId>`) and implement the table above; do
not rely on a task-local.

### 1.10 Timestamps: ORM, not database

- Every soft-deletable model has `created_at`/`updated_at` from `TimeAuditModel`.
- `User` has `created_at`, `updated_at`, **and** `date_joined`, all `auto_now_add`/
  `auto_now`; `User.last_active` defaults to `timezone.now` (a callable default, evaluated
  per instance).
- `UserRecentVisit.visited_at` is `auto_now=True`.
- `DeviceSession.start_time` is `auto_now_add=True`.
- `PageVersion.last_saved_at`, `IssueVersion.last_saved_at`,
  `IssueDescriptionVersion.last_saved_at`, `Account.last_connected_at`,
  `SocialLoginConnection.last_login_at`/`last_received_at` use
  `default=django.utils.timezone.now`.
- No DateTimeField in `db`/`license` has a database default. A Rust INSERT that omits
  `created_at`/`updated_at` will fail on `NOT NULL`, and an UPDATE that does not set
  `updated_at` will leave the old value (Django would refresh it on a full save).
- Some code explicitly passes `updated_at=timezone.now()` in column-only updates because
  `QuerySet.update()` does not run `auto_now`
  (`plane/app/views/workspace/member.py:134,176,217`,
  `plane/app/views/project/invite.py:172`,
  `plane/bgtasks/issue_automation_task.py:167`).

### 1.11 Models that are NOT soft-deletable

These have no `deleted_at`, no `SoftDeletionManager`, and their `delete()` is a real
DELETE (hard, with Django's cascade):

| model | file:line | table | manager | note |
| --- | --- | --- | --- | --- |
| `User` | `plane/db/models/user.py:56` | `users` | `django.contrib.auth.models.UserManager` | `AbstractBaseUser, PermissionsMixin`; UUID PK; `save()` override §2.1 |
| `Profile` | `plane/db/models/user.py:200` | `profiles` | plain `Manager` | `TimeAuditModel` only, own UUID PK; `user` OneToOne CASCADE |
| `Account` | `plane/db/models/user.py:271` | `accounts` | plain `Manager` | `TimeAuditModel` only, own UUID PK; `unique_together (provider, provider_account_id)` |
| `Session` | `plane/db/models/session.py:17` | `sessions` | `django.contrib.sessions.base_session.BaseSessionManager` | `session_key` CharField(128) PK; no `id`; `user_id` is a CharField, not an FK |

Everything else in `db` and `license` (92 models) is soft-deletable, including
`Integration` and `ProjectIdentifier` (which inherit `AuditModel` directly) and the
license models (`Instance`, `InstanceAdmin`, `InstanceConfiguration`, `ChangeLog`).

Deleting a `User` is a hard delete that cascades to a large set of rows: all
`created_by`/`updated_by` columns become `NULL` (SET_NULL), and every user-owned relation
with CASCADE is deleted (comments they authored, their API tokens, memberships, pages they
own, versions they own, favourites, notification preferences, …). Full list in §5.3.

`Profile`/`Account`/`Session` have no model hooks.

---

## 2. Per-model reference

Format: **class** — `file:line`, table, base classes, managers, `Meta`, then behaviour.
Models with no custom behaviour are one line.

`Meta.ordering` is listed because it determines the default `ORDER BY` whenever a query
does not specify one. Rust must apply the same default ordering on every list query.
Where a model section lists no explicit index, the table has only the implicit PK, FK and
unique indexes Django/Postgres create; explicit `Meta.indexes`/`UniqueConstraint`s are
listed for every model that has them. **No model in `db`/`license` defines `clean()`, and
`full_clean()`/`validate_unique()` are never called on a save** (grep in §A.5); the only
validation is DRF serializer validation of model validators (§B.3).

Unless stated otherwise, every soft-deletable model has the standard manager pair
`objects = SoftDeletionManager()` (filter `deleted_at IS NULL`) and
`all_objects = plane.Manager()` (no filter); they are not repeated below.

### 2.1 `plane/db/models/user.py`

#### `User` — `user.py:56`
Table `users`, ordering `("-created_at",)`. Base `AbstractBaseUser, PermissionsMixin`.
Manager `objects = UserManager()`. **Not soft-deletable.** UUID PK `uuid.uuid4`.
Unique: `username`, `email` (both DB unique, nullable email). `USERNAME_FIELD = "email"`,
`REQUIRED_FIELDS = ["username"]`.

Callable defaults: `last_active = timezone.now`. Auto fields: `date_joined`,
`created_at` (`auto_now_add`), `updated_at` (`auto_now`).

`save()` — `user.py:169-187`:
1. `self.email = self.email.lower().strip()` — unconditional; `email=None` raises
   `AttributeError` (column is nullable but every code path sets it).
2. `self.mobile_number = self.mobile_number` — no-op.
3. If `token_updated_at is not None`: `token = uuid.uuid4().hex + uuid.uuid4().hex`
   (64 chars) and `token_updated_at = timezone.now()`. This runs on **every** save while
   `token_updated_at` is set, so any full save rotates the token.
4. If `display_name` is empty: `email.split("@")[0]` (the `len(...)` test in the code is
   always true). The random 6-letter fallback in the expression is dead code.
5. If `is_superuser`: `is_staff = True`.
6. `super().save()` (Django `Model.save`; `User` has no BaseModel, so **no crum audit
   hook**, which is correct: it has no audit columns).

Properties used by serializers: `avatar_url` (`:143`), `cover_image_url` (`:154`),
`full_name` (`:165`, `"{first_name} {last_name}".strip()`).

`delete()`: hard delete (Model.delete). Cascades per §5.3.

#### `Profile` — `user.py:200`
Table `profiles`, ordering `("-created_at",)`, base `TimeAuditModel`, plain manager, own
UUID PK. OneToOne `user` → `users` CASCADE (unique). No save/delete/clean/property.
Callable defaults: `onboarding_step`→`get_default_onboarding()` (`user.py:25`: four false
flags `profile_complete`, `workspace_create`, `workspace_invite`, `workspace_join`);
`mobile_onboarding_step`→`get_mobile_default_onboarding()` (`:34`: `profile_complete`,
`workspace_create`, `workspace_join`); `product_tour`→`get_default_product_tour()`
(`:42`: `work_items`, `cycles`, `modules`, `intake`, `pages`, all false);
`background_color`→`plane.utils.color.get_random_color()`; `theme`/`goals` → `dict`.

#### `Account` — `user.py:271`
Table `accounts`, ordering `("-created_at",)`, base `TimeAuditModel`, plain manager, own
UUID PK. `unique_together ("provider", "provider_account_id")` — note there is no
`deleted_at` in the key because the model is not soft-deletable. No hooks. Callable
defaults: `last_connected_at`→`timezone.now`, `metadata`→`dict`.

### 2.2 `plane/db/models/workspace.py`

#### `Workspace` — `workspace.py:119`
Table `workspaces`, ordering `("-created_at",)`, base `BaseModel`. Soft-deletable.
Callable defaults: `background_color = get_random_color`
(`plane/utils/color.py:9`: `"#" + 6` chars drawn with `random.choices` from
`string.hexdigits` = `0123456789abcdefABCDEF`, so the result is **mixed case**).
`slug` is `SlugField(max_length=48, unique=True)`
with `slug_validator` (`workspace.py:114-116`) rejecting `RESTRICTED_WORKSPACE_SLUGS`
(`plane/utils/constants.py:5`, checked by DRF because it copies model-field validators).

`delete()` — **override**, `workspace.py:156-176`:
1. `result = super().delete(using=using, soft=soft, ...)` → `SoftDeleteModel.delete`
   (full save of `deleted_at`, enqueue cascade task).
2. If `soft` and `self.deleted_at` is set: `ts = int(self.deleted_at.timestamp())`;
   `self.slug = f"{self.slug}__{ts}"`; `self.save(update_fields=["slug"])`.
   `update_fields` means `updated_at`/`created_by`/`updated_by` are **not** written by this
   second save. The rename frees the slug for reuse.
   `slug` is `varchar(48)`; a slug longer than 36 characters makes this UPDATE fail with a
   Postgres `value too long` error *after* the row is already soft-deleted and the cascade
   task enqueued (suspected upstream bug — reported with this document).
3. Hard delete (`soft=False`) skips the slug rewrite.

Property: `logo_url` (`:145`) = `logo_asset.asset_url` if set, else `logo`, else `None`.

#### `WorkspaceBaseModel` — `workspace.py:185` (abstract)
Adds `workspace` FK → `workspaces` CASCADE (non-null) and `project` FK → `projects`
CASCADE (nullable). `save()` (`:192-195`): if `self.project` is set, assign
`self.workspace = self.project.workspace` before saving. Accessing `self.project` may
issue a query; `project.workspace` may issue a second one. This runs on every create and
update of every subclass (`Description`, `DescriptionVersion`, `Label`, `IssueView`,
`UserFavorite`, `DeployBoard`, `DraftIssue`, `DraftIssueAssignee`, `DraftIssueLabel`,
`DraftIssueModule`, `DraftIssueCycle`, `UserRecentVisit`).

#### `WorkspaceMember` — `workspace.py:198`
Table `workspace_members`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace", "member", "deleted_at")` + partial unique
`workspace_member_unique_workspace_member_when_deleted_at_null` on
`("workspace","member") WHERE deleted_at IS NULL`. No save/delete. Callable defaults:
`view_props`/`default_props`→`get_default_props()` (`workspace.py:22`: nested
`filters` (9 nulls), `display_filters` (the 7-key list view dict), `display_properties`
(13 true booleans)); `issue_props`→`get_issue_props()` (`:110`: `subscribed`, `assigned`,
`created`, `all_issues` = true); `getting_started_checklist`/`tips`/`explored_features`→`dict`.

#### `WorkspaceMemberInvite` — `workspace.py:234`
Table `workspace_member_invites`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("email","workspace","deleted_at")` + partial unique
`workspace_member_invite_unique_email_workspace_when_deleted_at_null`. No hooks. `token`
is a JWT written by the view (`plane/app/views/workspace/invite.py:95`).

#### `Team` — `workspace.py:261`
Table `teams`, ordering `("-created_at",)`, base `BaseModel`. Soft-deletable.
`unique_together ("name","workspace","deleted_at")` + partial unique
`team_unique_name_workspace_when_deleted_at_null`. Not re-exported from
`plane/db/models/__init__.py`, but registered because the module is imported. **No code in
`plane/` creates or queries it** (dead in CE); it still exists in the DB and is handled by
`hard_delete`'s final loop.

#### `WorkspaceTheme` — `workspace.py:286`
Table `workspace_themes`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","name","deleted_at")` + partial unique
`workspace_theme_unique_workspace_name_when_deleted_at_null`. No hooks.

#### `WorkspaceUserProperties` — `workspace.py:310`
Table `workspace_user_properties`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","user","deleted_at")` + partial unique
`workspace_user_properties_unique_workspace_user_when_deleted_at_null`. No hooks.
Callable defaults: `filters`→`workspace.get_default_filters()` (9 nulls);
`display_filters`→`workspace.get_default_display_filters()` (`:76`, **nested** under
`{"display_filters": {...}}`); `display_properties`→`workspace.get_default_display_properties()`
(`:90`, **nested** under `{"display_properties": {...}}`); `rich_filters`→`dict`.
`navigation_control_preference` defaults to `"ACCORDION"`.

#### `WorkspaceUserLink` — `workspace.py:354`
Table `workspace_user_links`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
No hooks. `metadata`→`dict`. Because `WorkspaceBaseModel.project` is nullable, a link can
be workspace-level or project-level; project delete cascades to it.

#### `WorkspaceHomePreference` — `workspace.py:374`
Table `workspace_home_preferences`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","user","key","deleted_at")` + partial unique
`workspace_user_home_preferences_unique_workspace_user_key_when_deleted_at_null`.
No hooks. `config`→`dict`. `key` values are the `HomeWidgetKeys` choices.

#### `WorkspaceUserPreference` — `workspace.py:417`
Table `workspace_user_preferences`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","user","key","deleted_at")` + partial unique
`workspace_user_preferences_unique_workspace_user_key_when_deleted_at_null`.
No hooks. `key` values are the `UserPreferenceKeys` choices.

### 2.3 `plane/db/models/project.py`

#### `Project` — `project.py:68`
Table `projects`, ordering `("-created_at",)`, base `ChangeTrackerMixin, BaseModel`,
`TRACKED_FIELDS = ["timezone"]`. Soft-deletable. Callable default `logo_props`→`dict`
(and `description_text`/`description_html` JSON have no default → `NULL`).

`unique_together` (DB-level, includes the nullable `deleted_at`):
`("identifier","workspace","deleted_at")` and `("name","workspace","deleted_at")`.
Partial unique constraints:

- `project_unique_identifier_workspace_when_deleted_at_null` on
  `("identifier","workspace") WHERE deleted_at IS NULL`
- `project_unique_name_workspace_when_deleted_at_null` on
  `("name","workspace") WHERE deleted_at IS NULL`

Model validator `FORBIDDEN_IDENTIFIER_CHARS_PATTERN` (`project.py:145`) is used by the
serializers (not by `save()`).

`__init__` — `project.py:124-127`: sets `self.is_timezone_provided = kwargs.get("timezone") is not None`
**before** `super().__init__`. Only keyword arguments count (Django's `from_db` loads
positionally, so this is False for loaded instances, which is fine because the flag is
only read on create).

`save()` — `project.py:193-227`, in order:
1. `self.identifier = self.identifier.strip().upper()` — on every save, create or update.
2. `is_creating = self._state.adding`.
3. If creating and `not is_timezone_provided`: `workspace = Workspace.objects.get(id=self.workspace_id)`
   (one query) and `self.timezone = workspace.timezone`. Note `Workspace.objects` is
   soft-delete filtered: a soft-deleted workspace raises `DoesNotExist`.
4. `update_fields = kwargs.get("update_fields")`; if not `None`, materialise to a tuple
   and write it back to `kwargs` (so the membership test below cannot exhaust a generator).
5. `timezone_persisted = "timezone" not in self.get_deferred_fields() and (update_fields is None or "timezone" in update_fields)`.
6. If creating **or** the timezone is not being persisted → `super().save()` and return.
   (So an update with `update_fields=["name"]` never retimes anything.)
7. `old_timezone = self.old_values["timezone"]` if tracked; otherwise
   `Project.objects.values_list("timezone", flat=True).get(pk=self.pk)` (one query;
   soft-delete filtered).
8. If unchanged → `super().save()` and return.
9. Otherwise `with transaction.atomic(): super().save(); self._retime_intake_snoozes(old_timezone)`.

`_retime_intake_snoozes(old_timezone)` — `project.py:169-191`, called only from step 9:
- `pytz.timezone(old)`, `pytz.timezone(new)`.
- Iterates `IntakeIssue.objects.select_for_update().filter(project_id=self.id, status=SNOOZED, snoozed_till__isnull=False).only("id","snoozed_till")`
  (row locks; soft-delete filtered).
- For each: `snoozed_till = project_day_start(snoozed_till.astimezone(old_tz).date(), new_tz)`
  — i.e. keep the calendar day in the old zone, store midnight of that day in the new
  zone (`plane/utils/timezone_converter.py:98-100` = `tz.localize(datetime.combine(day, time.min))`).
- `IntakeIssue.objects.bulk_update(retimed, ["snoozed_till"])` — **column-only**: no
  `IntakeIssue.save()` (so no re-flooring), no `updated_at`, no audit columns.

`super().save()` in steps 6/8/9 resolves through `ChangeTrackerMixin.save` →
`BaseModel.save` (audit hook).

Property: `cover_image_url` (`:129`) = `cover_image_asset.asset_url` if set, else
`cover_image`, else `None`.

`delete()`: inherited soft delete (full `Project.save()` runs: identifier re-uppercased,
timezone compared — likely unchanged — then the cascade task soft-deletes all children).

#### `ProjectBaseModel` — `project.py:230` (abstract)
Adds `project` FK → `projects` CASCADE (non-null, `related_name="project_%(class)s"`) and
`workspace` FK → `workspaces` CASCADE (non-null, `related_name="workspace_%(class)s"`).

`save()` — `project.py:237-239`: `self.workspace = self.project.workspace` on **every**
save (create and update). `self.project` triggers a query if not cached; `project.workspace`
may trigger a second. Then `super().save()` (`BaseModel.save`). Every subclass below pays
this cost and must be ported with the workspace derived from the project (or from the
already-known value) rather than trusting the caller's `workspace_id`.

#### `ProjectMember` — `project.py:260`
Table `project_members`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("project","member","deleted_at")` + partial unique
`project_member_unique_project_member_when_deleted_at_null`. `member` is nullable
(CASCADE), `role` default 5.

`save()` — `project.py:276-292`:
1. If `self._state.adding and self.member`:
   - `min_sort_order = ProjectUserProperty.objects.filter(workspace_id=self.project.workspace_id, user=self.member).aggregate(Min("sort_order"))`
     (soft-delete filtered; `self.project` triggers a query).
   - `ProjectUserProperty.objects.create(workspace_id=..., project=self.project, user=self.member, sort_order=(min - 10000 if min is not None else 65535))`.
     This is a normal create: `ProjectBaseModel.save` re-derives workspace, `BaseModel.save`
     sets `created_by` to the request user. The unique constraint on
     `(user, project, deleted_at)` may raise if a row exists (e.g. a re-added member) —
     the hook does not `get_or_create`.
2. `super().save()`.

Callable defaults: `view_props`/`default_props`→`project.get_default_props()` (`project.py:39`:
`{"filters": 9 nulls, "display_filters": 7-key dict}`),
`preferences`→`get_default_preferences()` (`:64`:
`{"pages": {"block_display": True}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}`),
`sort_order` 65535, `is_active` True.

#### `ProjectMemberInvite` — `project.py:242`
Table `project_member_invites`, ordering `("-created_at",)`, base `ProjectBaseModel`.
No unique constraint, no hooks. `token` (JWT) set by the view. `role` default 5.

#### `ProjectIdentifier` — `project.py:314`
Table `project_identifiers`, ordering `("-created_at",)`, base **`AuditModel`** (not
`BaseModel`). **Implicit `id = BigAutoField`** (serial), not a UUID —
`plane/db/migrations/0001_initial.py:1270-1276`.
`unique_together ("name","workspace","deleted_at")` + partial unique
`unique_name_workspace_when_deleted_at_null` on `("name","workspace") WHERE deleted_at IS NULL`.
`project` is a OneToOne → `projects` CASCADE; `workspace` FK is nullable.

**No audit hook**: `save()` is `django.db.models.Model.save`, so `created_by`/`updated_by`
are **not** set from the request user and `created_by_id=`/`disable_auto_set_user=` are
not accepted. Writes: `plane/app/serializers/project.py:108` and
`plane/api/serializers/project.py:293` (`objects.create`), and a queryset soft delete in
`plane/app/views/project/base.py:470`. `delete()` still comes from `SoftDeleteModel`.

#### `ProjectDeployBoard` — `project.py:350`
Table `project_deploy_boards`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("project","anchor")` (no `deleted_at`). Callable defaults:
`anchor`→`project.get_anchor()` (`project.py:334`: `uuid4().hex`), `views`→
`get_default_views()` (`:338`: `list`, `kanban`, `calendar`, `gantt`, `spreadsheet` all
true). `intake` FK SET_NULL. No hooks. Legacy/deploy-board model (as opposed to
`DeployBoard`).

#### `ProjectPublicMember` — `project.py:370`
Table `project_public_members`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("project","member","deleted_at")` + partial unique
`project_public_member_unique_project_member_when_deleted_at_null`. No hooks.

#### `ProjectUserProperty` — `project.py:392`
Table `project_user_properties`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("user","project","deleted_at")` + partial unique
`project_user_property_unique_user_project_when_deleted_at_null`. No hooks.
Callable defaults (imported from `.issue` inside the class body):
`filters`→`issue.get_default_filters()` (`issue.py:47`, 9 nulls);
`display_filters`→`issue.get_default_display_filters()` (`issue.py:61`, 7-key dict);
`display_properties`→`issue.get_default_display_properties()` (`issue.py:73`, 13 true
booleans); `preferences`→`project.get_default_preferences()`; `rich_filters`→`dict`.
`ProjectMember.save` creates one of these (§2.3), and many views `get_or_create` them.

### 2.4 `plane/db/models/issue.py`

#### `Issue` — `issue.py:104`
Table `issues`, ordering `("-created_at",)`, base `ChangeTrackerMixin, ProjectBaseModel`,
`TRACKED_FIELDS = ["state_id"]`. Managers: `issue_objects = IssueManager()` (default
manager), `objects` (soft-deletion), `all_objects`.

Fields with behaviour: `sequence_id` Integer default 1; `sort_order` Float default
`65535`; `description_html` default `"<p></p>"`; `description_json` default `dict`;
`priority` default `"none"`; `is_draft` False; `archived_at` DateField null;
`completed_at` DateTime null; `state` FK CASCADE nullable; `parent` self-FK CASCADE;
`estimate_point`/`type` SET_NULL; `assignees` M2M through `IssueAssignee`; `labels` M2M
through `IssueLabel`. No `unique_together`, no `UniqueConstraint`, no model indexes beyond
FK/PK indexes.

`save()` — `issue.py:180-222`, exact order:
1. `_ensure_default_state()` (`:228-238`):
   - If `self.state is not None` → return. Accessing `self.state` on an instance with
     `state_id=None` returns `None`; with a `state_id` it may fetch.
   - `State.objects.filter(~Q(is_triage=True), project=self.project, default=True).first()`
     (StateManager excludes `group='triage'` and soft-deleted rows).
   - If none: `State.objects.filter(~Q(is_triage=True), project=self.project).first()` —
     both queries use `StateManager`, so states with `group='triage'` and soft-deleted
     states are excluded; `~Q(is_triage=True)` is an additional filter.
   - Assigns `self.state` (or leaves it `None` if the project has no states).
2. `kwargs = self._sync_completed_at(kwargs)` (`:240-255`):
   - If `self.state` is falsy → return unchanged.
   - If not adding and `not self.has_changed("state_id")` → return unchanged.
   - If `self.state.group == "completed"` → `completed_at = timezone.now()`, else
     `completed_at = None`. On create this runs even though `has_changed` is not
     consulted, so an issue created in a completed state gets `completed_at=now`.
   - If `update_fields` is not None, add `"completed_at"` to it.
3. If `self._state.adding`:
   - `with transaction.atomic():`
   - `lock_key = convert_uuid_to_integer(self.project.id)`
     (`plane/utils/uuid.py:19-26`: `int.from_bytes(sha256(str(uuid).encode()).digest()[:8], "big", signed=True)`).
     **Exact same key required** to interlock with Django during coexistence.
   - `SELECT pg_advisory_xact_lock(%s)` on the project key.
   - `last_sequence = IssueSequence.objects.filter(project=self.project).aggregate(Max("sequence"))["largest"]`
     (soft-delete filtered; `IssueSequence` has no soft-delete rows unless deleted).
   - `self.sequence_id = last_sequence + 1 if last_sequence else 1`. **Any caller-supplied
     `sequence_id` is overwritten on create.** Per project, monotonic, never reused while
     the `IssueSequence` row survives.
   - `self.description_stripped = None if description_html in ("", None) else strip_tags(description_html)`
     using `plane.utils.html_processor.strip_tags` (HTMLParser with
     `convert_charrefs=True`: tags removed, **entities decoded**, comments/scripts
     contents kept as text). Different from `django.utils.html.strip_tags` used by
     `Description`/`DescriptionVersion`, which removes tags but leaves entities encoded.
   - `largest_sort_order = Issue.objects.filter(project=self.project, state=self.state).aggregate(Max("sort_order"))["largest"]`
     — `Issue.objects` is the **plain soft-deletion manager**, so archived, triage and
     draft issues count toward the maximum.
   - If not None → `self.sort_order = largest + 10000`; otherwise keep the supplied value
     (default 65535).
   - `super().save()` → `ChangeTrackerMixin.save` → `ProjectBaseModel.save` (re-derive
     workspace) → `BaseModel.save` (audit).
   - `IssueSequence.objects.create(issue=self, sequence=self.sequence_id, project=self.project)`
     — one INSERT; `ProjectBaseModel.save` derives the workspace; `BaseModel.save` sets
     audit columns from the request user.
4. Else (update): recompute `description_stripped` exactly as above, then
   `super().save()`. No sequence/sort-order work.

`delete()`: inherited soft delete. Because the inherited delete calls `self.save()`,
deleting an Issue runs steps 1, 2 and 4 of `save()` again (recompute
`description_stripped`, possibly recompute `completed_at` if the tracked state changed in
memory) before writing `deleted_at`. The cascade task then soft-deletes children.

Write-path notes:
- `IssueCreateSerializer.create` (`plane/app/serializers/issue.py:209`) does
  `Issue.objects.create(...)` (hook runs), then `IssueAssignee`/`IssueLabel`
  `bulk_create` with `created_by_id`/`updated_by_id` copied from the saved issue
  (bulk = no hooks).
- `IssueCreateSerializer.update` (`:286`) does `IssueAssignee.objects.filter(issue=...).delete()`
  (bulk **soft** delete of the through rows), `bulk_create` the new ones, sets
  `instance.updated_at = timezone.now()` and calls `super().update` → full `issue.save()`.
- `api/serializers/issue.py:199` additionally resolves a default `IssueType`.
- Bulk paths that bypass this hook are in §4.

#### `IssueBlocker` — `issue.py:258`
Table `issue_blockers`, ordering `("-created_at",)`, base `ProjectBaseModel`. Two FKs to
`issues` (`block`, `blocked_by`), both CASCADE. No constraint, no hooks, **no code in
`plane/` writes it** (dead in CE).

#### `IssueRelation` — `issue.py:296`
Table `issue_relations`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","related_issue","deleted_at")` + partial unique
`issue_relation_unique_issue_related_issue_when_deleted_at_null` on
`("issue","related_issue") WHERE deleted_at IS NULL`. `relation_type` default
`IssueRelationChoices.BLOCKED_BY`. The class-level `_RELATION_PAIRS` / `_REVERSE_MAPPING`
(`:283-293`) map `blocked_by↔blocking`, `relates_to↔relates_to`, `duplicate↔duplicate`,
`start_before↔start_after`, `finish_before↔finish_after`, `implemented_by↔implements`;
they are used by the views to synthesise the reverse side (there is **no** second row).
No hooks. Creation is a bulk_create in `plane/app/views/issue/relation.py:274` and
`plane/api/views/issue.py:2744` (no save hook).

#### `IssueMention` — `issue.py:323`
Table `issue_mentions`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","mention","deleted_at")` + partial unique
`issue_mention_unique_issue_mention_when_deleted_at_null`. No hooks. Created by
`plane/bgtasks/notification_task.py:49` with `bulk_create`.

#### `IssueAssignee` — `issue.py:345`
Table `issue_assignees`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","assignee","deleted_at")` + partial unique
`issue_assignee_unique_issue_assignee_when_deleted_at_null`. No hooks. Through model of
`Issue.assignees`; writes are `bulk_create` in serializers and `.delete()` (soft) +
`bulk_create` on update.

#### `IssueLink` — `issue.py:371`
Table `issue_links`, ordering `("-created_at",)`, base `ProjectBaseModel`. `metadata`→`dict`.
No hooks, no unique constraint (duplicate URLs are rejected in the serializer, not the
model).

#### `IssueAttachment` — `issue.py:398`
Table `issue_attachments`, ordering `("-created_at",)`, base `ProjectBaseModel`.
Legacy: attachments are `FileAsset` today; `IssueAttachment` has no writer in `plane/`
(only the legacy serializer). `asset` FileField `upload_to=get_upload_path`
(`issue.py:387`: `f"{instance.workspace.id}/{uuid4().hex}-{sanitized filename}"`, filename
fallback `uuid4().hex`), validator `file_size` (`:392`, max `settings.FILE_SIZE_LIMIT`,
default 5 MiB). `attributes`→`dict`. No hooks.

#### `IssueActivity` — `issue.py:415`
Table `issue_activities`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`issue` FK → `issues` **DO_NOTHING**, nullable; `issue_comment` FK → `issue_comments`
**DO_NOTHING**, nullable; `actor` FK → users SET_NULL. `attachments` ArrayField default
`list`; `verb` default `"created"`. No hooks.

`DO_NOTHING` means Django never deletes/nulls these rows when the issue or comment is
deleted; the DB constraint still exists. See §5.5.

#### `IssueComment` — `issue.py:451`
Table `issue_comments`, ordering `("-created_at",)`, base
`ChangeTrackerMixin, ProjectBaseModel`,
`TRACKED_FIELDS = ["comment_stripped", "comment_json", "comment_html"]`.
`description` is a **OneToOne** to `descriptions` CASCADE, nullable (unique).
`attachments` ArrayField default `list`; `comment_json`→`dict`; `comment_html` default
`"<p></p>"`; `access` default `"INTERNAL"`; `actor` FK CASCADE nullable; `parent` self-FK
CASCADE nullable. No unique constraints.

`save()` — `issue.py:481-530`:
1. `self.comment_stripped = strip_tags(self.comment_html) if comment_html != "" else ""`
   — `plane.utils.html_processor.strip_tags`; note the `else ""` means an empty HTML
   yields `""`, not `None` (unlike `Issue`/`Page`/`Description`).
2. `is_creating = self._state.adding`.
3. Builds `description_defaults` **before** saving:
   `workspace_id`, `project_id`, `created_by_id`, `updated_by_id` taken from the
   **current in-memory instance**, `description_stripped`, `description_json`,
   `description_html`.
   On a DRF create `created_by`/`updated_by` are read-only and not populated yet (crum's
   `BaseModel.save` has not run), so the `Description` row is created with **NULL audit
   columns**. The unit tests pass `created_by=...` explicitly and therefore do not cover
   that path.
4. `with transaction.atomic():`
   - `super().save()` (→ ChangeTracker captures `_changes_on_save` → ProjectBaseModel →
     BaseModel → Model).
   - If creating **or** `description_id` is NULL: `Description.objects.create(**defaults)`
     (a full `Description.save`: strips HTML again, `ProjectBaseModel` re-derives
     workspace, BaseModel sets audit from the request user), assign `self.description_id`,
     then `super().save(update_fields=["description_id"])` (column-only second write; no
     `updated_at`).
   - Else: `changed_fields = {desc_field: value for comment_field, desc_field in {comment_html→description_html, comment_stripped→description_stripped, comment_json→description_json} if comment_field in self._changes_on_save}`;
     if any: `Description.objects.filter(pk=self.description_id).update(**changed_fields, updated_by_id=self.updated_by_id, updated_at=self.updated_at)`
     — **column-only**, bypasses `Description.save`, uses the comment's (already
     recomputed by BaseModel.save) `updated_by_id`/`updated_at`.
5. `delete()`: inherited soft delete → `IssueComment.save()` runs again (step 1 etc.).

#### `IssueLabel` — `issue.py:543`
Table `issue_labels`, ordering `("-created_at",)`, base `ProjectBaseModel`. Through model
of `Issue.labels`. No constraint (the unique pair is enforced only by the serializer's
`.delete()` + `bulk_create`), no hooks.

#### `IssueSequence` — `issue.py:557`
Table `issue_sequences`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`issue` FK → `issues` **SET_NULL**, nullable (the row is kept after the issue is
deleted); `sequence` PositiveBigInteger default 1 (db_index); `deleted` Boolean default
False (unused by every query — the aggregate does not filter it). No hooks. Only writer is
`Issue.save` (one row per issue) plus the seed/dummy tasks.

#### `IssueSubscriber` — `issue.py:574`
Table `issue_subscribers`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","subscriber","deleted_at")` + partial unique
`issue_subscriber_unique_issue_subscriber_when_deleted_at_null`. No hooks. Created with
`bulk_create` in `notification_task.py:455` and `issue_activities_task.py:431`.

#### `IssueReaction` — `issue.py:600`
Table `issue_reactions`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","actor","reaction","deleted_at")` + partial unique
`issue_reaction_unique_issue_actor_reaction_when_deleted_at_null`. No hooks.

#### `CommentReaction` — `issue.py:627`
Table `comment_reactions`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("comment","actor","reaction","deleted_at")` + partial unique
`comment_reaction_unique_comment_actor_reaction_when_deleted_at_null`. No hooks.

#### `IssueVote` — `issue.py:654`
Table `issue_votes`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","actor","deleted_at")` + partial unique
`issue_vote_unique_issue_actor_when_deleted_at_null`. `vote` default 1. No hooks.

#### `IssueVersion` — `issue.py:677`
Table `issue_versions`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`last_saved_at`→`timezone.now`; `assignees`/`labels`/`modules` ArrayField default `list`;
`properties`/`meta`→`dict`; `activity` FK SET_NULL; `owned_by` FK CASCADE. No `save()`
override. Writes: `bulk_create` in `plane/bgtasks/issue_version_sync.py:214` and
`IssueVersion.log_issue_version(issue, user)` (`issue.py:736-779`), which reads
`CycleIssue`, `IssueAssignee`, `IssueLabel`, `Module` (all soft-delete filtered, so
soft-deleted links are skipped), snapshots the issue fields (`parent_id`, `state_id`,
`estimate_point_id`, …), sets `last_saved_at=now`, `owned_by=user`, and returns
`True`/`False` swallowing exceptions with `log_exception`. The classmethod's
`cls.objects.create` is a full save (ProjectBaseModel + BaseModel audit; `created_by` =
request user if called in a request, else NULL).

#### `IssueDescriptionVersion` — `issue.py:782`
Table `issue_description_versions`, ordering none (`Meta.ordering` absent → database
order). No `unique_together`. `last_saved_at`→`timezone.now`, `description_json`→`dict`,
`description_binary` nullable, `owned_by` FK CASCADE. No `save()` override. Writes:
`bulk_create` in `plane/bgtasks/issue_description_version_sync.py:105` and
`IssueDescriptionVersion.log_issue_description_version(issue, user)` (`:800-822`), which
copies `description_*` plus `created_by_id`/`updated_by_id` **from the issue**, sets
`owned_by=user`, `last_saved_at=now`.

### 2.5 `plane/db/models/state.py`

#### `State` — `state.py:79`
Table `states`, ordering `("sequence",)` — **the only model besides `Estimate`,
`EstimatePoint`, `Intake`, `ProjectIssueType` with non-`-created_at` ordering**.
Base `ProjectBaseModel`. Managers
`objects = StateManager()`, `triage_objects = TriageStateManager()`,
`all_state_objects = Manager()`, plus inherited `all_objects`.
`unique_together ("name","project","deleted_at")` + partial unique
`state_unique_name_project_when_deleted_at_null`. Fields: `slug` SlugField(100) blank,
`sequence` Float default 65535, `group` default `"backlog"`, `is_triage` False,
`default` False, `color` required, `name` required.

`save()` — `state.py:117-126`:
1. `self.slug = slugify(self.name)` on **every** save (create and update) using
   `django.template.defaultfilters.slugify` = NFKD-normalise → ASCII → lowercase →
   remove `[^\w\s-]` → collapse `[-\s]+` to `-` → strip leading/trailing `-_`
   (`django/utils/text.py:448-465`). Not truncated to the 100-char column.
2. If `self._state.adding`: `last = State.objects.filter(project=self.project).aggregate(Max("sequence"))["largest"]`;
   if not None → `self.sequence = last + 15000`; if None keep the supplied/default 65535.
   `State.objects` excludes triage states and soft-deleted rows.
3. `super().save()`.

`DEFAULT_STATES` (`state.py:24-62`) is the seed list used by project creation (six states
with explicit sequences 15000…65000, `Backlog` default); those are created with
`State.objects.bulk_create` (`plane/app/views/project/base.py:282`,
`plane/api/views/project.py:261`), which **bypasses `save()`**, so default states have an
**empty `slug`** and the explicit sequences.

`__str__` only; no delete override (inherited soft delete + task).

### 2.6 `plane/db/models/label.py`

#### `Label` — `label.py:11`
Table `labels`, ordering `("-created_at",)`, base `WorkspaceBaseModel` (so `project` is
nullable → workspace-level labels exist). No `unique_together`; two partial unique
constraints: `unique_name_when_project_null_and_not_deleted` on `("name") WHERE project IS NULL AND deleted_at IS NULL`,
and `unique_project_name_when_not_deleted` on `("project","name") WHERE project IS NOT NULL AND deleted_at IS NULL`.
Fields: `sort_order` Float 65535, `parent` self-FK CASCADE nullable, `external_*`.

`save()` — `label.py:46-54`:
1. If adding: `last = Label.objects.filter(project=self.project).aggregate(Max("sort_order"))["largest"]`
   (`self.project` may be `None` → aggregates workspace-level labels); if not None →
   `sort_order = last + 10000`; else default 65535.
2. `super().save()`.
No update-time behaviour.

### 2.7 `plane/db/models/cycle.py`

#### `Cycle` — `cycle.py:60`
Table `cycles`, ordering `("-created_at",)`, base `ProjectBaseModel`. No uniqueness
constraint at all. Fields: `sort_order` 65535, `view_props`/`progress_snapshot`/`logo_props`
→`dict`, `owned_by` FK CASCADE, `timezone` default `"UTC"`, `version` Integer default 1,
`archived_at` nullable.

`save()` — `cycle.py:88-97`: if adding,
`smallest = Cycle.objects.filter(project=self.project).aggregate(Min("sort_order"))["smallest"]`;
if not None → `sort_order = smallest - 10000`; else 65535. Then `super().save()`.

#### `CycleIssue` — `cycle.py:104`
Table `cycle_issues`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","cycle","deleted_at")` + partial unique
`cycle_issue_when_deleted_at_null` on `("cycle","issue") WHERE deleted_at IS NULL`.
No hooks. Writes are `bulk_create`/`bulk_update` (see §4) and `.delete()` (soft).

#### `CycleUserProperties` — `cycle.py:130`
Table `cycle_user_properties`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("cycle","user","deleted_at")` + partial unique
`cycle_user_properties_unique_cycle_user_when_deleted_at_null`. No hooks. Callable
defaults: `filters`/`display_filters`/`display_properties` from `cycle.py:16/30/42`
(9 nulls; 7-key dict; 13 true booleans); `rich_filters`→`dict`.

### 2.8 `plane/db/models/module.py`

#### `Module` — `module.py:67`
Table `modules`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("name","project","deleted_at")` + partial unique
`module_unique_name_project_when_deleted_at_null`. Fields: `sort_order` 65535,
`status` default `"planned"`, `lead` FK SET_NULL nullable, `members` M2M through
`ModuleMember`, `view_props`/`logo_props`→`dict`, `archived_at` nullable.

`save()` — `module.py:115-124`: if adding,
`smallest = Module.objects.filter(project=self.project).aggregate(Min("sort_order"))["smallest"]`;
if not None → `sort_order = smallest - 10000`; else 65535. Then `super().save()`.

#### `ModuleMember` — `module.py:130`
Table `module_members`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("module","member","deleted_at")` + partial unique
`module_member_unique_module_member_when_deleted_at_null`. No hooks; through model of
`Module.members`, written with `bulk_create` in the serializers.

#### `ModuleIssue` — `module.py:152`
Table `module_issues`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue","module","deleted_at")` + partial unique
`module_issue_unique_issue_module_when_deleted_at_null`. No hooks; `bulk_create`/`bulk_update`.

#### `ModuleLink` — `module.py:174`
Table `module_links`, ordering `("-created_at",)`, base `ProjectBaseModel`.
No constraints. `metadata`→`dict`. No hooks.

#### `ModuleUserProperties` — `module.py:190`
Table `module_user_properties`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("module","user","deleted_at")` + partial unique
`module_user_properties_unique_module_user_when_deleted_at_null`. No hooks. Callable
defaults `module.py:14/28/40` (9 nulls; 7-key dict; 13 true booleans); `rich_filters`→`dict`.

### 2.9 `plane/db/models/intake.py`

#### `Intake` — `intake.py:16`
Table `intakes`, ordering `("name",)`, base `ProjectBaseModel`.
`unique_together ("name","project","deleted_at")` + partial unique
`intake_unique_name_project_when_deleted_at_null`. `is_default` False,
`view_props`/`logo_props`→`dict`. No hooks.

#### `IntakeIssue` — `intake.py:54`
Table `intake_issues`, ordering `("-created_at",)`, base `ChangeTrackerMixin, ProjectBaseModel`,
`TRACKED_FIELDS = ["snoozed_till"]`. `status` Integer default `-2` (PENDING;
`-1` rejected, `0` snoozed, `1` accepted, `2` duplicate), `source` default `"IN_APP"`,
`duplicate_to` FK SET_NULL nullable, `extra`→`dict`. Index
`intake_issue_snoozed_till_idx` on `("snoozed_till") WHERE status = 0`.

`save()` — `intake.py:100-110`:
1. If `snoozed_till` is set **and** (adding or `has_changed("snoozed_till")`):
   - `project_tz = pytz.timezone(Project.objects.values_list("timezone", flat=True).get(id=self.project_id))`
     (soft-delete filtered; a missing/soft-deleted project raises `DoesNotExist`).
   - `self.snoozed_till = project_day_start(self.snoozed_till.astimezone(project_tz).date(), project_tz)`
     — the calendar day in the project's timezone, stored as midnight in that timezone.
2. `super().save()`.

The serializer pre-normalises a bare `"YYYY-MM-DD"` with
`snooze_day_start(value, project_id)` (`plane/utils/timezone_converter.py:103-112`) before
the field reaches `save()` (`plane/app/serializers/intake.py:50-53`,
`plane/api/serializers/intake.py:120-123`). `Project.save` re-times existing snoozes when
the project timezone changes.

`unsnooze_expired_intake_issues` (`plane/bgtasks/issue_automation_task.py:164-174`) is a
**column-only** update: `status=PENDING, snoozed_till=NULL, updated_at=now` for
`status=SNOOZED AND snoozed_till <= now`; the comment says "nothing listens for
SNOOZED → PENDING".

### 2.10 `plane/db/models/draft.py`

#### `DraftIssue` — `draft.py:16`
Table `draft_issues`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
No uniqueness constraints. Fields: `sort_order` 65535, `priority` default `"none"`,
`state` FK CASCADE nullable, `parent`/`estimate_point`/`type` SET_NULL nullable,
`description_*`, `assignees` through `DraftIssueAssignee`, `labels` through
`DraftIssueLabel`, `completed_at` nullable.

`save()` — `draft.py:84-132`:
1. If `self.state is None`: look up the project's default non-triage state
   (`State.objects.filter(~Q(is_triage=True), project=..., default=True).first()`), else a
   random non-triage state (`.first()`), and assign it. Catches only `ImportError`.
2. Else (state set): if `self.state.group == "completed"` → `completed_at = now`, else
   `completed_at = None`. **Runs on every save**, not only when the state changed.
3. If adding: compute `description_stripped` (`plane.utils.html_processor.strip_tags`,
   `None` for empty/None HTML); `largest = DraftIssue.objects.filter(project=..., state=...).aggregate(Max("sort_order"))`;
   if not None → `sort_order = largest + 10000`; `super().save()`.
4. Else: recompute `description_stripped`; `super().save()`.

#### `DraftIssueAssignee` — `draft.py:139`
Table `draft_issue_assignees`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`unique_together ("draft_issue","assignee","deleted_at")` + partial unique
`draft_issue_assignee_unique_issue_assignee_when_deleted_at_null`. No hooks.

#### `DraftIssueLabel` — `draft.py:165`
Table `draft_issue_labels`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
No constraints, no hooks.

#### `DraftIssueModule` — `draft.py:179`
Table `draft_issue_modules`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`unique_together ("draft_issue","module","deleted_at")` + partial unique
`module_draft_issue_unique_issue_module_when_deleted_at_null`. No hooks.

#### `DraftIssueCycle` — `draft.py:201`
Table `draft_issue_cycles`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`unique_together ("draft_issue","cycle","deleted_at")` + partial unique
`draft_issue_cycle_when_deleted_at_null`. No hooks.

### 2.11 `plane/db/models/page.py`

#### `Page` — `page.py:23`
Table `pages`, ordering `("-created_at",)`, base `BaseModel` (not `WorkspaceBaseModel`;
`workspace` is a direct FK CASCADE). Fields: `description_html` default `"<p></p>"`,
`sort_order` Float default `DEFAULT_SORT_ORDER = 65535`, `access` default 0
(0=Public, 1=Private), `view_props`→`get_view_props()` (`page.py:19`:
`{"full_width": False}`), `logo_props`→`dict`, `labels` M2M through `PageLabel`,
`projects` M2M through `ProjectPage`, `parent` self-FK CASCADE nullable, `archived_at`
DateField nullable, `is_locked`/`is_global` False, `owned_by` FK CASCADE.

`save()` — `page.py:70-77`: `description_stripped = None if html in ("", None) else plane.utils.html_processor.strip_tags(html)`
on every save, then `super().save()`. No sort-order hook (it stays 65535 unless the caller
sets it).

`delete()`: inherited. The `destroy` view also runs
`Page.objects.filter(parent_id=page_id, ...).update(parent=None)` (column-only, no hooks,
no `updated_at`) before deleting (`plane/app/views/page/base.py:394-401`).

#### `PageLog` — `page.py:80`
Table `page_logs`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("page","transaction")`. `transaction`→`uuid.uuid4` (per row). Indexes:
`pagelog_entity_type_idx` (`entity_type`), `pagelog_entity_id_idx` (`entity_identifier`),
`pagelog_entity_name_idx` (`entity_name`), `pagelog_type_id_idx`
(`entity_type`,`entity_identifier`), `pagelog_name_id_idx`
(`entity_name`,`entity_identifier`). No hooks. Written with `bulk_create`
(`page_transaction_task.py:133`).

#### `PageLabel` — `page.py:120`
Table `page_labels`, ordering `("-created_at",)`, base `BaseModel`. No constraints, no
hooks. Written with `bulk_create` (`serializers/page.py:93,112`).

#### `ProjectPage` — `page.py:135`
Table `project_pages`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("project","page","deleted_at")` + partial unique
`project_page_unique_project_page_when_deleted_at_null`. No hooks.

#### `PageVersion` — `page.py:158`
Table `page_versions`, ordering `("-created_at",)`, base `BaseModel`.
`last_saved_at`→`timezone.now`; `description_json`→`dict`; `sub_pages_data`→`dict`.
No constraints.

`save()` — `page.py:175-182`: recompute `description_stripped` with
`plane.utils.html_processor.strip_tags` (same rule as `Page`) then `super().save()`.
Rows are pruned by `delete_page_versions` (keeps the newest 20 per page).

### 2.12 `plane/db/models/description.py`

#### `Description` — `description.py:10`
Table `descriptions`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`description_json`→`dict`; `description_html` default `"<p></p>"`; `description_binary`
nullable. No constraints.

`save()` — `description.py:22-29`: `description_stripped = None if html in ("", None) else **django.utils.html.strip_tags**(html)`
— **different function** from the other stripped fields: it removes `<...>` repeatedly
but does **not** decode entities. Then `super().save()`.

#### `DescriptionVersion` — `description.py:32`
Table `description_versions`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
Same fields plus `description` FK → `descriptions` CASCADE. `save()` (`:49-56`) uses
`django.utils.html.strip_tags` again.

### 2.13 `plane/db/models/sticky.py`

#### `Sticky` — `sticky.py:16`
Table `stickies`, ordering `("-created_at",)`, base `BaseModel`.
`description`→`dict`; `description_html` default `"<p></p>"`; `logo_props`→`dict`;
`sort_order` 65535; `workspace`/`owner` FKs CASCADE.

`save()` — `sticky.py:38-54`:
1. Recompute `description_stripped` with `plane.utils.html_processor.strip_tags`
   (`None` for empty/None HTML).
2. If adding: `largest = Sticky.objects.filter(workspace=self.workspace).aggregate(Max("sort_order"))["largest"]`;
   if not None → `sort_order = largest + 10000`.
3. `super().save()`.

### 2.14 `plane/db/models/view.py`

#### `IssueView` — `view.py:58`
Table `issue_views`, ordering `("-created_at",)`, base `WorkspaceBaseModel` (project may
be NULL for workspace-level views). No constraints. Fields: `query` JSONField **required**
(no default), `filters`→`dict`, `display_filters`/`display_properties` from `view.py:28/40`
(7-key dict; 13 true booleans), `rich_filters`→`dict`, `access` default 1 (1=Public for
this model), `sort_order` 65535, `logo_props`→`dict`, `owned_by` FK CASCADE,
`archived_at` nullable.

`save()` — `view.py:79-95`:
1. `params = self.filters`; `self.query = issue_filters(params, "POST") if params else {}`
   (`plane/utils/issue_filters.py`; translates the legacy filter JSON into a dict of
   Django lookups). Runs on every save (create and update).
2. If adding: if `self.project` → `max = IssueView.objects.filter(project=self.project).aggregate(Max("sort_order"))`;
   else → `IssueView.objects.filter(workspace=self.workspace, project__isnull=True).aggregate(...)`.
   If not None → `sort_order = max + 10000`; else 65535.
3. `super().save()`.

### 2.15 `plane/db/models/favorite.py`

#### `UserFavorite` — `favorite.py:14`
Table `user_favorites`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`unique_together ("entity_type","user","entity_identifier","deleted_at")` + partial
unique `user_favorite_unique_entity_type_entity_identifier_user_when_deleted_at_null`.
Indexes `fav_entity_type_idx`, `fav_entity_identifier_idx`, `fav_entity_idx`.
Fields: `sequence` Float 65535, `is_folder` False, `parent` self-FK CASCADE nullable.

`save()` — `favorite.py:52-65`: if adding:
- if `self.project` → `max = UserFavorite.objects.filter(workspace=self.project.workspace).aggregate(Max("sequence"))`;
- else → `max = UserFavorite.objects.filter(workspace=self.workspace).aggregate(...)`;
- if not None → `sequence = max + 10000`; else 65535.
Then `super().save()`.

### 2.16 `plane/db/models/notification.py`

#### `Notification` — `notification.py:13`
Table `notifications`, ordering `("-created_at",)`, base `BaseModel`.
`workspace` CASCADE, `project` CASCADE nullable, `triggered_by` SET_NULL nullable,
`receiver` CASCADE, `data`/`message` nullable JSON, `entity_identifier` nullable UUID,
`message_html` default `"<p></p>"`, `message_stripped` nullable, `read_at`/`snoozed_till`/
`archived_at` nullable. Nine indexes: `notif_entity_identifier_idx`,
`notif_entity_name_idx`, `notif_read_at_idx`, `notif_entity_idx`
(`receiver`,`read_at`), `notif_receiver_status_idx`
(`receiver`,`workspace`,`read_at`,`created_at`), `notif_receiver_entity_idx`
(`receiver`,`workspace`,`entity_name`,`read_at`), `notif_receiver_state_idx`
(`receiver`,`workspace`,`snoozed_till`,`archived_at`), `notif_receiver_sender_idx`
(`receiver`,`workspace`,`sender`), `notif_entity_lookup_idx`
(`workspace`,`entity_identifier`,`entity_name`). No hooks. Writes are `bulk_create`
(`notification_task.py:669`) and `bulk_update` of `read_at`
(`plane/app/views/notification/base.py:292`).

#### `UserNotificationPreference` — `notification.py:81`
Table `user_notification_preferences`, ordering `("-created_at",)`, base `BaseModel`.
`user` CASCADE, `workspace`/`project` CASCADE nullable. Booleans `property_change`,
`state_change`, `comment`, `mention`, `issue_completed` all default True. No unique
constraint (one row per user in practice). No hooks. **Created by the `post_save` signal
on `User`** (§3); also updated through the serializer (full save).

#### `EmailNotificationLog` — `notification.py:121`
Table `email_notification_logs`, ordering `("-created_at",)`, base `BaseModel`.
`receiver`/`triggered_by` CASCADE; `processed_at`/`sent_at` nullable; `old_value`/
`new_value` CharField(300) nullable. No hooks. Written with `bulk_create`
(`notification_task.py:670`) and column updates for `processed_at`/`sent_at`
(`email_notification_task.py:84,287`).

### 2.17 `plane/db/models/asset.py`

#### `FileAsset` — `asset.py:32`
Table `file_assets`, ordering `("-created_at",)`, base `BaseModel`.
CASCADE FKs: `user`, `workspace`, `draft_issue`, `project`, `issue`, `comment`, `page`
(all nullable). Fields: `asset` FileField `max_length=800` with
`get_upload_path` (`asset.py:20`: workspace-scoped `"{workspace.id}/{uuid4().hex}-{sanitized}"`,
or `"user-{uuid4().hex}-{filename}"` when `workspace_id` is null) and `file_size` validator
(max `FILE_SIZE_LIMIT`, default 5 MiB); `attributes`→`dict`; `size` Float 0;
`is_deleted`/`is_archived`/`is_uploaded` False; `storage_metadata`→`dict`; `entity_type`
values are `EntityTypeContext` (`asset.py:37-47`). Indexes `asset_entity_type_idx`,
`asset_entity_identifier_idx`, `asset_entity_idx`, `asset_asset_idx`.

No `save()`/`delete()` override. **Deletion is manual**: views set
`is_deleted=True` and `deleted_at=now` and call
`save(update_fields=["is_deleted","deleted_at"])` (e.g.
`plane/app/views/asset/v2.py:80,239,492,580,717`, `plane/app/views/asset/base.py:64,80,108`,
`plane/api/views/asset.py:244`). A plain `FileAsset.objects.filter(...).delete()` (soft)
does **not** set `is_deleted`; the two flags are independent.

Property `asset_url` (`asset.py:83-104`) — the URL shape depends on `entity_type`:
`/api/assets/v2/static/{id}/` for workspace logo/user avatar/user cover/project cover;
`/api/assets/v2/workspaces/{workspace.slug}/projects/{project_id}/issues/{issue_id}/attachments/{id}/`
for issue attachments; `/api/assets/v2/workspaces/{workspace.slug}/projects/{project_id}/{id}/`
for issue/comment/page/draft description assets; `None` otherwise. Uses
`self.workspace.slug` (query if not loaded) and can emit `/…/None/` when the ids are
missing.

Method `is_project_accessible_to(user)` (`asset.py:106-146`) — permission helper used by
asset views: `True` when `project_id` is NULL; otherwise requires an active
`ProjectMember` in the asset's workspace+project; a project GUEST of a project with
`guest_view_all_features=False` may only read assets of issues they created (via
`Issue.objects`, so archived/draft issues stay visible to their creator). Not a hook, but
part of every asset read path.

### 2.18 `plane/db/models/webhook.py`

#### `Webhook` — `webhook.py:34`
Table `webhooks`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","url","deleted_at")` + partial unique
`webhook_url_unique_url_when_deleted_at_null`. `url` URLField(1024) with validators
`validate_schema` (only http/https) and `validate_domain` (rejects exactly `localhost`
and `127.0.0.1`; other loopback/private hosts pass) — `webhook.py:21-31`; run by DRF
because model validators are copied. `secret_key`→`generate_token()`
(`webhook.py:17`: `"plane_wh_" + uuid4().hex`), unique not enforced; `version` default
`"v1"`; event booleans default False; `is_active` True. No hooks.

#### `WebhookLog` — `webhook.py:65`
Table `webhook_logs`, ordering `("-created_at",)`, base `BaseModel`. `webhook` is a
**UUIDField**, not an FK (no referential integrity). `retry_count` default 0. No hooks.

#### `ProjectWebhook` — `webhook.py:94`
Table `project_webhooks`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("project","webhook","deleted_at")` + partial unique
`project_webhook_unique_project_webhook_when_deleted_at_null`. No hooks.

### 2.19 `plane/db/models/estimate.py`

#### `Estimate` — `estimate.py:18`
Table `estimates`, ordering `("name",)`, base `ProjectBaseModel`.
`unique_together ("name","project","deleted_at")` + partial unique
`estimate_unique_name_project_when_deleted_at_null`. `type` default `"categories"`,
`last_used` False. No hooks.

#### `EstimatePoint` — `estimate.py:43`
Table `estimate_points`, ordering `("value",)`, base `ProjectBaseModel`.
`estimate` FK CASCADE; `key` Integer default 0 with `MinValueValidator(0)`; `value`
required. No hooks. `Issue.estimate_point`/`DraftIssue.estimate_point` are SET_NULL.
Writes are `bulk_create`/`bulk_update` (`plane/app/views/estimate/base.py:82,141,261`,
`plane/api/views/estimate.py:227`).

### 2.20 `plane/db/models/issue_type.py`

#### `IssueType` — `issue_type.py:14`
Table `issue_types`, ordering none. Base `BaseModel`, `workspace` FK CASCADE.
`logo_props`→`dict`; `is_epic`/`is_default` False; `is_active` True; `level` Float 0.
No hooks. Created/updated through `plane/app/views/issue/type.py` (full saves).

#### `ProjectIssueType` — `issue_type.py:35`
Table `project_issue_types`, ordering `("project","issue_type")`, base
`ProjectBaseModel`. `unique_together ("project","issue_type","deleted_at")` + partial
unique `project_issue_type_unique_project_issue_type_when_deleted_at_null`. `level`
PositiveInteger 0, `is_default` False. No hooks.

### 2.21 `plane/db/models/analytic.py`

#### `AnalyticView` — `analytic.py:11`
Table `analytic_views`, ordering `("-created_at",)`, base `BaseModel`. `query` JSON
required; `query_dict`→`dict`; `workspace` CASCADE. No hooks.

### 2.22 `plane/db/models/api.py`

#### `APIToken` — `api.py:23`
Table `api_tokens`, ordering `("-created_at",)`, base `BaseModel`.
`label`→`generate_label_token()` (`api.py:15`: `uuid4().hex`); `token`→`generate_token()`
(`api.py:19`: `"plane_api_" + uuid4().hex`), **unique**, db_index; `user` FK CASCADE;
`workspace` FK CASCADE nullable; `user_type` default 0 (human); `is_active` True.
No hooks beyond BaseModel. `last_used` is updated by
`plane/app/middleware/api_authentication.py:42` with
`save(update_fields=["last_used"])`.

#### `APIActivityLog` — `api.py:49`
Table `api_activity_logs`, ordering `("-created_at",)`, base `BaseModel`.
`token_identifier` CharField (not an FK); `response_code` PositiveInteger; `ip_address`
GenericIPAddress nullable. No hooks. Created inside the `process_logs` Celery task
(`plane/bgtasks/logger_task.py:25`), so `created_by`/`updated_by` are NULL.

### 2.23 `plane/db/models/exporter.py`

#### `ExporterHistory` — `exporter.py:24`
Table `exporters`, ordering `("-created_at",)`, base `BaseModel`. `token`→
`generate_token()` (`exporter.py:20`: `uuid4().hex`), unique; `type` default
`"issue_exports"`; `status` default `"queued"`; `provider` required (json/csv/xlsx);
`project` is `ArrayField(UUIDField(default=uuid.uuid4), blank=True, null=True)` — the
default is a **scalar UUID** for an array column and is never used in practice because
the view always passes `project=[...]` (`plane/app/views/exporter/base.py:42`);
`initiated_by` FK CASCADE; `filters` nullable JSON; `rich_filters`→`dict`. No hooks.
Status changes use `save(update_fields=[...])` (`export_task.py:125,146,206,230`).

### 2.24 `plane/db/models/importer.py`

#### `Importer` — `importer.py:13`
Table `importers`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`service` required (github/jira); `status` default `"queued"`; `initiated_by` CASCADE;
`token` FK → `api_tokens` CASCADE; `metadata`/`config`/`data`→`dict`; `imported_data`
nullable. No constraints, no hooks.

### 2.25 `plane/db/models/deploy_board.py`

#### `DeployBoard` — `deploy_board.py:19`
Table `deploy_boards`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`unique_together ("entity_name","entity_identifier","deleted_at")` + partial unique
`deploy_board_unique_entity_name_entity_identifier_when_deleted_at_null` (note: this
partial constraint has no `workspace` column). `anchor`→`get_anchor()` (`:15`:
`uuid4().hex`), unique; `intake` FK SET_NULL nullable; `view_props`→`dict`;
`is_activity_enabled` True; the rest False. No hooks.

### 2.26 `plane/db/models/device.py`

#### `Device` — `device.py:11`
Table `devices`, ordering none (`Meta.ordering` absent). Base `BaseModel`. `user` CASCADE;
`device_type` required (ANDROID/IOS/WEB/DESKTOP); `is_active` True. No hooks. **No code in
`plane/` writes it.**

#### `DeviceSession` — `device.py:30`
Table `device_sessions`, ordering none. Base `BaseModel`. `device` CASCADE; `session` FK →
`sessions` CASCADE; `is_active` True; `start_time` `auto_now_add`; `end_time` nullable.
No hooks. No writer in `plane/`.

### 2.27 `plane/db/models/recent_visit.py`

#### `UserRecentVisit` — `recent_visit.py:22`
Table `user_recent_visits`, ordering `("-created_at",)`, base `WorkspaceBaseModel`.
`entity_identifier` nullable UUID; `entity_name` CharField(30); `user` CASCADE;
`visited_at` **`auto_now=True`** (every full save refreshes it). No hooks. Note: many
views hard-delete these with `.delete(soft=False)` (§4).

### 2.28 `plane/db/models/session.py`

#### `Session` — `session.py:17`
Table `sessions`, base `AbstractBaseSession`. PK `session_key` CharField(128);
`session_data` Text; `expire_date` DateTime (indexed by Django's abstract base);
`user_id` **CharField(50), nullable, db_index — not a ForeignKey**; `device_info`
JSONField nullable default `None`. Manager `BaseSessionManager` (its `save()` deletes
expired sessions; `AbstractBaseSession` has `get_session_store_class`). No custom hooks.

`SessionStore` (`:30-56`) overrides `_get_new_session_key()` to generate 128 chars from
`[a-z0-9]` and sets `user_id` from `_auth_user_id` and `device_info` from the session dict
in `create_model_instance`. Rust must reproduce the key format, the payload encoding
(§RUST_REWRITE "Sessions") and the `expire_date` attribute.

### 2.29 `plane/db/models/social_connection.py`

#### `SocialLoginConnection` — `social_connection.py:14`
Table `social_login_connections`, ordering `("-created_at",)`, base `BaseModel`.
`medium` default `None` (choices Google/Github/GitLab/Jira); `last_login_at`/
`last_received_at`→`timezone.now`; `token_data`/`extra_data` nullable JSON. No hooks.
No writer in current code (dead in CE).

### 2.30 `plane/db/models/integration/`

#### `Integration` — `integration/base.py:16`
Table `integrations`, ordering `("-created_at",)`, base **`AuditModel`** (not
`BaseModel`). Own `id` UUIDField `uuid4` PK. `provider` **unique**; `network` default 1;
`description`/`metadata`→`dict`; `verified` False. **No audit hook** (plain
`Model.save`), same caveat as `ProjectIdentifier`. No writer in `plane/` (dead in CE).

#### `WorkspaceIntegration` — `integration/base.py:41`
Table `workspace_integrations`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("workspace","integration")` (no `deleted_at`; soft-deleted rows still
occupy the pair). `workspace`/`actor`/`integration`/`api_token` all CASCADE.
`metadata`/`config`→`dict`. No hooks. No writer.

#### `GithubRepository` — `integration/github.py:14`
Table `github_repositories`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`repository_id` BigInteger; `config`→`dict`; `url` nullable. No constraints, no hooks, no
writer.

#### `GithubRepositorySync` — `integration/github.py:32`
Table `github_repository_syncs`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("project","repository")`; `repository` is a **OneToOne** CASCADE;
`label` SET_NULL nullable; `credentials`→`dict`; `actor` CASCADE;
`workspace_integration` CASCADE. No hooks, no writer.

#### `GithubIssueSync` — `integration/github.py:54`
Table `github_issue_syncs`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("repository_sync","issue")`; `issue` CASCADE; `repository_sync`
CASCADE. No hooks, no writer.

#### `GithubCommentSync` — `integration/github.py:73`
Table `github_comment_syncs`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("issue_sync","comment")`; `comment` CASCADE; `issue_sync` CASCADE.
No hooks, no writer.

#### `SlackProjectSync` — `integration/slack.py:14`
Table `slack_project_syncs`, ordering `("-created_at",)`, base `ProjectBaseModel`.
`unique_together ("team_id","project")`; `workspace_integration` CASCADE; `data`→`dict`.
No hooks, no writer.

### 2.31 `plane/license/models/instance.py`

#### `Instance` — `instance.py:22`
Table `instances`, ordering `("-created_at",)`, base `BaseModel`. `instance_id` unique;
`edition` default `"PLANE_COMMUNITY"`; `last_checked_at` DateTime **required (no default)**
— every INSERT must supply it; telemetry/setup booleans default True/False per model.
No hooks. Writes: `register_instance` command, license admin views (`instance.save()`,
full save → BaseModel audit).

#### `InstanceAdmin` — `instance.py:53`
Table `instance_admins`, ordering `("-created_at",)`, base `BaseModel`.
`unique_together ("instance","user")`; `user` SET_NULL nullable
(`related_name "instance_owner"`); `instance` CASCADE; `role` default 20. No hooks. Deletion through the
API is a **soft** delete (`InstanceAdmin.objects.filter(...).delete()`,
`plane/license/api/views/admin.py:87`).

#### `InstanceConfiguration` — `instance.py:72`
Table `instance_configurations`, ordering `("-created_at",)`, base `BaseModel`. `key`
unique; `value` Text nullable; `category` required; `is_encrypted` False. No hooks.
Bulk value updates use `bulk_update`
(`plane/license/api/views/configuration.py:56`) and a `Case/When` `QuerySet.update`
(`:79`).

#### `ChangeLog` — `instance.py:86`
Table `changelogs`, ordering `("-created_at",)`, base `BaseModel`. `tags`→`list`;
`release_date` nullable. Not exported from `license/models/__init__.py` but registered.
No hooks, no writer.

---

## 3. Signals

There is exactly **one** Django signal receiver in `apps/api/plane/` (verified by the
grep in Appendix A): no `pre_save`, `post_delete`, `pre_delete`, `m2m_changed`, and no
`.connect()` on a Django signal anywhere.

### S01 — `create_user_notification`

- **Receiver**: `plane/db/models/user.py:298-312`
- **Signal / sender**: `django.db.models.signals.post_save`, `sender=User`
- **Trigger**: every successful INSERT of a `users` row (`User.objects.create`,
  `UserManager.create_user/create_superuser`, OAuth signup's `user.save()`, seed bot
  creation, `loaddata` fixtures).
- **Behaviour**: if `created and not instance.is_bot`:
  `UserNotificationPreference.objects.create(user=instance, property_change=True, state_change=True, comment=True, mention=True, issue_completed=True)`.
  A normal `Model.save()` on the preference, so it gets `created_at`/`updated_at`, a UUID
  and `created_by`/`updated_by` from crum (NULL during signup because `request.user` is
  anonymous, NULL in the seed task).
- **Not checked**: `raw` (fixtures also create a preference row), exceptions (a failure
  aborts the user save when inside a transaction).
- Rust equivalent: `plane_db::hooks::user::after_insert(tx, user)` — insert the preference
  row in the same transaction as the user INSERT, skipping bot users.

Celery's `@after_setup_logger.connect` / `@after_setup_task_logger.connect`
(`plane/celery.py:105,113`) are **Celery** signals for logger configuration, not model
signals; they have no database effect.

---

## 4. Other implicit write behaviour

### 4.1 `bulk_create` / `bulk_update` — `save()` is NOT called

`bulk_create` and `bulk_update` write rows with one INSERT/UPDATE per batch. They do
**not** call the model's `save()`, do **not** run `BaseModel.save` (so no
`created_by`/`updated_by` from the request user) and do **not** send `post_save`.

Precise timestamp behaviour (Django 5.2):

- `bulk_create`: the insert compiler calls `field.pre_save(obj, add=True)` for each field
  (`django/db/models/sql/compiler.py:1743-1750`), so `auto_now_add` **and** `auto_now`
  fields **are** populated with `timezone.now()` and primary-key defaults (`uuid.uuid4`)
  **are** evaluated (`QuerySet._prepare_for_bulk_create`,
  `django/db/models/query.py:675-691`). What is missing is `save()`, the audit logic and
  the signal. `created_by`/`updated_by` stay whatever the caller put on the instances
  (usually `NULL`, sometimes explicitly copied from a parent or `request.user`).
- `bulk_update`: no `pre_save` at all. It builds a `CASE WHEN pk … THEN <in-memory value>`
  for each named field (`query.py:895-925`), so it writes exactly the values currently on
  the instances and only those columns. `updated_at` is **not** refreshed, even if it is
  in the field list (the in-memory value is the stale one loaded from the database). No
  `save()`, no signals.

Call sites (production code, `plane/` minus tests/migrations). "hook bypassed" names the
hooks from §2 that a normal save would have run:

| file:line | call | hook bypassed |
| --- | --- | --- |
| `plane/api/serializers/issue.py:207` | `IssueAssignee.objects.bulk_create` | none |
| `plane/api/serializers/issue.py:248` | `IssueLabel.objects.bulk_create` | none |
| `plane/api/serializers/issue.py:280` | `IssueAssignee.objects.bulk_create` | none |
| `plane/api/serializers/issue.py:301` | `IssueLabel.objects.bulk_create` | none |
| `plane/api/serializers/module.py:105` | `ModuleMember.objects.bulk_create` | none |
| `plane/api/serializers/module.py:150` | `ModuleMember.objects.bulk_create` | none |
| `plane/api/views/cycle.py:1013` | `CycleIssue.objects.bulk_create` | none |
| `plane/api/views/cycle.py:1047` | `CycleIssue.objects.bulk_update` | none |
| `plane/api/views/estimate.py:227` | `EstimatePoint.objects.bulk_create` | none |
| `plane/api/views/issue.py:2744` | `IssueRelation.objects.bulk_create` | none |
| `plane/api/views/issue.py:2774` | `?.objects.?` | none |
| `plane/api/views/module.py:766` | `ModuleIssue.objects.bulk_create` | none |
| `plane/api/views/module.py:768` | `ModuleIssue.objects.bulk_update` | none |
| `plane/api/views/project.py:261` | `State.objects.bulk_create` | **`State.save`** (empty `slug`) |
| `plane/app/serializers/draft.py:188` | `DraftIssueAssignee.objects.bulk_create` | none |
| `plane/app/serializers/draft.py:204` | `DraftIssueLabel.objects.bulk_create` | none |
| `plane/app/serializers/draft.py:230` | `DraftIssueModule.objects.bulk_create` | none |
| `plane/app/serializers/draft.py:262` | `DraftIssueAssignee.objects.bulk_create` | none |
| `plane/app/serializers/draft.py:279` | `DraftIssueLabel.objects.bulk_create` | none |
| `plane/app/serializers/draft.py:308` | `DraftIssueModule.objects.bulk_create` | none |
| `plane/app/serializers/issue.py:226` | `IssueAssignee.objects.bulk_create` | none |
| `plane/app/serializers/issue.py:267` | `IssueLabel.objects.bulk_create` | none |
| `plane/app/serializers/issue.py:299` | `IssueAssignee.objects.bulk_create` | none |
| `plane/app/serializers/issue.py:320` | `IssueLabel.objects.bulk_create` | none |
| `plane/app/serializers/module.py:104` | `ModuleMember.objects.bulk_create` | none |
| `plane/app/serializers/module.py:76` | `ModuleMember.objects.bulk_create` | none |
| `plane/app/serializers/page.py:112` | `PageLabel.objects.bulk_create` | none |
| `plane/app/serializers/page.py:93` | `PageLabel.objects.bulk_create` | none |
| `plane/app/views/cycle/issue.py:264` | `CycleIssue.objects.bulk_create` | none |
| `plane/app/views/cycle/issue.py:299` | `CycleIssue.objects.bulk_update` | none |
| `plane/app/views/estimate/base.py:141` | `EstimatePoint.objects.bulk_update` | none |
| `plane/app/views/estimate/base.py:261` | `EstimatePoint.objects.bulk_update` | none |
| `plane/app/views/estimate/base.py:82` | `EstimatePoint.objects.bulk_create` | none |
| `plane/app/views/issue/archive.py:341` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/app/views/issue/base.py:1183` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/app/views/issue/label.py:97` | `Label.objects.bulk_create` | **`Label.save`** (`sort_order=65535`) |
| `plane/app/views/issue/relation.py:274` | `IssueRelation.objects.bulk_create` | none |
| `plane/app/views/issue/sub_issue.py:238` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/app/views/module/issue.py:227` | `ModuleIssue.objects.bulk_create` | none |
| `plane/app/views/module/issue.py:276` | `ModuleIssue.objects.bulk_create` | none |
| `plane/app/views/notification/base.py:292` | `Notification.objects.bulk_update` | none |
| `plane/app/views/project/base.py:282` | `State.objects.bulk_create` | **`State.save`** (empty `slug`) |
| `plane/app/views/project/invite.py:114` | `ProjectMemberInvite.objects.bulk_create` | none |
| `plane/app/views/project/invite.py:174` | `ProjectMember.objects.bulk_create` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/app/views/project/invite.py:188` | `ProjectUserProperty.objects.bulk_create` | none |
| `plane/app/views/project/member.py:105` | `ProjectMember.objects.bulk_update` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/app/views/project/member.py:144` | `ProjectMember.objects.bulk_create` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/app/views/project/member.py:146` | `ProjectUserProperty.objects.bulk_create` | none |
| `plane/app/views/user/base.py:334` | `ProjectMember.objects.bulk_update` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/app/views/user/base.py:336` | `WorkspaceMember.objects.bulk_update` | none |
| `plane/app/views/workspace/draft.py:328` | `ModuleIssue.objects.bulk_create` | none |
| `plane/app/views/workspace/home.py:45` | `WorkspaceHomePreference.objects.bulk_create` | none |
| `plane/app/views/workspace/invite.py:112` | `WorkspaceMemberInvite.objects.bulk_create` | none |
| `plane/app/views/workspace/invite.py:297` | `WorkspaceMember.objects.bulk_create` | none |
| `plane/app/views/workspace/user_preference.py:39` | `WorkspaceUserPreference.objects.bulk_create` | none |
| `plane/authentication/utils/workspace_project_join.py:21` | `WorkspaceMember.objects.bulk_create` | none |
| `plane/authentication/utils/workspace_project_join.py:45` | `WorkspaceMember.objects.bulk_create` | none |
| `plane/authentication/utils/workspace_project_join.py:59` | `ProjectMember.objects.bulk_create` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/bgtasks/dummy_data_task.py:107` | `State.objects.bulk_create` | **`State.save`** (empty `slug`) |
| `plane/bgtasks/dummy_data_task.py:130` | `Label.objects.bulk_create` | **`Label.save`** (`sort_order=65535`) |
| `plane/bgtasks/dummy_data_task.py:188` | `Cycle.objects.bulk_create` | **`Cycle.save`** (`sort_order`) |
| `plane/bgtasks/dummy_data_task.py:218` | `Module.objects.bulk_create` | **`Module.save`** (`sort_order`) |
| `plane/bgtasks/dummy_data_task.py:241` | `Page.objects.bulk_create` | **`Page.save`** (`description_stripped`) |
| `plane/bgtasks/dummy_data_task.py:243` | `ProjectPage.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:264` | `PageLabel.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:324` | `Issue.objects.bulk_create` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/bgtasks/dummy_data_task.py:326` | `IssueSequence.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:340` | `IssueActivity.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:361` | `IntakeIssue.objects.bulk_create` | **`IntakeIssue.save`** (snooze flooring) |
| `plane/bgtasks/dummy_data_task.py:388` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/bgtasks/dummy_data_task.py:413` | `IssueAssignee.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:436` | `IssueLabel.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:454` | `CycleIssue.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:484` | `ModuleIssue.objects.bulk_create` | none |
| `plane/bgtasks/dummy_data_task.py:66` | `ProjectMember.objects.bulk_create` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/bgtasks/issue_activities_task.py:1630` | `IssueActivity.objects.bulk_create` | none |
| `plane/bgtasks/issue_activities_task.py:431` | `IssueSubscriber.objects.bulk_create` | none |
| `plane/bgtasks/issue_automation_task.py:135` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/bgtasks/issue_automation_task.py:72` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/bgtasks/issue_description_version_sync.py:105` | `IssueDescriptionVersion.objects.bulk_create` | none |
| `plane/bgtasks/issue_version_sync.py:214` | `IssueVersion.objects.bulk_create` | none |
| `plane/bgtasks/notification_task.py:455` | `IssueSubscriber.objects.bulk_create` | none |
| `plane/bgtasks/notification_task.py:49` | `IssueMention.objects.bulk_create` | none |
| `plane/bgtasks/notification_task.py:669` | `Notification.objects.bulk_create` | none |
| `plane/bgtasks/notification_task.py:670` | `EmailNotificationLog.objects.bulk_create` | none |
| `plane/bgtasks/page_transaction_task.py:133` | `PageLog.objects.bulk_create` | none |
| `plane/bgtasks/workspace_seed_task.py:116` | `ProjectMember.objects.bulk_create` | **`ProjectMember.save`** (no `ProjectUserProperty`) |
| `plane/bgtasks/workspace_seed_task.py:130` | `ProjectUserProperty.objects.bulk_create` | none |
| `plane/db/management/commands/copy_issue_comment_to_description.py:44` | `Description.objects.bulk_create` | **`Description.save`** (`description_stripped`) |
| `plane/db/management/commands/copy_issue_comment_to_description.py:51` | `IssueComment.objects.bulk_update` | **`IssueComment.save`** (description sync) |
| `plane/db/management/commands/create_dummy_data.py:42` | `WorkspaceMember.objects.bulk_create` | none |
| `plane/db/management/commands/fix_duplicate_sequences.py:90` | `Issue.objects.bulk_update` | **`Issue.save`** (sequence/sort/stripped/completed_at) |
| `plane/db/management/commands/fix_duplicate_sequences.py:91` | `IssueSequence.objects.bulk_update` | none |
| `plane/db/models/project.py:191` | `IntakeIssue.objects.bulk_update` | **`IntakeIssue.save`** (snooze flooring) |
| `plane/license/api/views/configuration.py:56` | `InstanceConfiguration.objects.bulk_update` | none |
| `plane/space/serializer/issue.py:318` | `IssueAssignee.objects.bulk_create` | none |
| `plane/space/serializer/issue.py:345` | `IssueLabel.objects.bulk_create` | none |
| `plane/space/serializer/issue.py:374` | `IssueAssignee.objects.bulk_create` | none |
| `plane/space/serializer/issue.py:391` | `IssueLabel.objects.bulk_create` | none |
| `plane/utils/cycle_transfer_issues.py:458` | `CycleIssue.objects.bulk_update` | none |
| `plane/utils/filters/filter_migrations.py:61` | `model_class.objects.bulk_update` | none |

Two related observations:

- `ProjectMember.objects.bulk_create` is the reason the invite/join views create
  `ProjectUserProperty` rows themselves. If Rust uses `bulk_create`-style inserts for
  memberships, it must also insert the user-property rows; if it calls the per-row hook, it
  must **not** also insert them (duplicate unique-key error).
- `State.objects.bulk_create` for `DEFAULT_STATES` is why default states have `slug=""`.
  A Rust project-create that runs `State.save` per row would produce different `slug`
  values (a visible difference in state serializers that include `slug`).

### 4.2 `QuerySet.update()` — column-only writes

No `F()` expression is used in any `.update()` in the repo (verified: every `.update(`
line and the following two lines were checked; the `F()` import sites are only
annotations/order_by). Real DB `.update()` calls that bypass `save()`:

| file:line | update | notes |
| --- | --- | --- |
| `plane/db/mixins.py:51` | `delete(soft=True)` → `update(deleted_at=now)` | the bulk soft delete |
| `plane/db/models/issue.py:528` | `Description.objects.filter(pk=...).update(changed, updated_by_id=..., updated_at=...)` | inside `IssueComment.save`; column-only |
| `plane/db/models/project.py:191` | `IntakeIssue.objects.bulk_update(["snoozed_till"])` | in `Project.save` |
| `plane/app/views/page/base.py:397` | `Page.objects.filter(...).update(parent=None)` | children keep `updated_at` |
| `plane/app/views/state/base.py:122` | `State.objects.filter(project_id=...).update(default=Case(When(pk=state.pk, then=True), default=False))` | "one UPDATE" to keep exactly one default |
| `plane/app/views/estimate/base.py:221` | `issues.update(estimate_point_id=new_estimate_id)` | **bypasses `Issue.save`**; the view sends activities itself |
| `plane/app/views/workspace/base.py:165` | `Profile.objects.filter(last_workspace_id=id).update(last_workspace_id=None)` | Profile has no hooks |
| `plane/app/views/workspace/member.py:134,176,217` | `WorkspaceMember...update(role=..., is_active=False, updated_at=now)` | explicit `updated_at`; audit columns untouched |
| `plane/app/views/workspace/invite.py:292` | `WorkspaceMember...update(...)` | reactivating an invite |
| `plane/app/views/project/invite.py:172` | `ProjectMember...update(is_active=True, role=..., updated_by=user, updated_at=now)` | explicit `updated_by` |
| `plane/app/views/asset/v2.py:796,803,811,816,822,954` | `FileAsset...update(...)` | binding assets; `update(is_uploaded=True)` |
| `plane/app/views/workspace/draft.py:360` | `FileAsset...update(issue_id=..., entity_type=..., draft_issue_id=None)` | draft → issue conversion |
| `plane/app/views/asset/base.py` | (uses `.save(update_fields)` instead) | — |
| `plane/space/views/asset.py:300` | `FileAsset...update(comment_id=...)` | binding comment images |
| `plane/api/serializers/state.py:32` | `State.objects.filter(project_id=...).exclude(pk=...).update(default=False)` | after saving a new default |
| `plane/bgtasks/deletion_task.py:61` | `related_queryset.update(fk=None)` | SET_NULL branch of the soft cascade |
| `plane/bgtasks/exporter_expired_task.py:53` | `ExporterHistory...update(url=None)` | — |
| `plane/bgtasks/copy_s3_object.py:121` | `FileAsset.objects.filter(pk__in=...).update(is_uploaded=True)` | after duplicating S3 objects |
| `plane/bgtasks/email_notification_task.py:84,287` | `EmailNotificationLog...update(processed_at/sent_at)` | — |
| `plane/bgtasks/webhook_task.py:356` | `Webhook...update(is_active=False)` | disables a failing webhook |
| `plane/bgtasks/issue_automation_task.py:167` | `IntakeIssue...update(status=PENDING, snoozed_till=None, updated_at=now)` | unsnooze |
| `plane/utils/filters/filter_migrations.py:126` | `model_class.objects...update(rich_filters={})` | only called from migration 0107 |
| `plane/db/management/commands/create_project_member.py:57` | `ProjectMember...update(...)` | — |
| `plane/license/api/views/configuration.py:79` | `InstanceConfiguration...update(value=Case(...))` | SMTP toggle |

### 4.3 Explicit `instance.save(update_fields=[...])`

These still run the model's `save()` hook, but only write the listed columns (plus any
column the hook adds, e.g. `Issue._sync_completed_at` adds `completed_at`). Important
sites:

- `plane/app/views/page/base.py:357` — `page.save(update_fields=["parent"])`: `Page.save`
  still recomputes `description_stripped` (not written).
- `plane/app/views/asset/v2.py:80,230,239,286,471,492,580,701,717`,
  `plane/app/views/asset/base.py:64,80,108` — `FileAsset` `update_fields` (`is_deleted`,
  `deleted_at`, `is_uploaded`, `attributes`).
- `plane/app/middleware/api_authentication.py:42` — `api_token.save(update_fields=["last_used"])`.
- `plane/bgtasks/issue_activities_task.py:812,838,879,913,949,1582` —
  `issue.save(update_fields=["updated_at"])`: running `Issue.save` just to bump
  `updated_at`; audit attributes are assigned in memory but not written.
- `plane/bgtasks/export_task.py:125,146,206,230` — `ExporterHistory.save(update_fields=["status",...])`.
- `plane/utils/cycle_transfer_issues.py:432` — `current_cycle.save(update_fields=["progress_snapshot"])`.
- `plane/app/views/page/base.py:599` and the page views at `:250,262,283` — full
  `page.save()`.
- `plane/bgtasks/workspace_seed_task.py:113,206,239,293,376,387,436,472` — full saves with
  `created_by_id` + `disable_auto_set_user=True`.

### 4.4 M2M `.add()` / `.set()`

Django implements `ManyRelatedManager.add()`/`set()` with
`through._default_manager.bulk_create(...)` (Django 5.2
`related_descriptors.py:1522,1551`), so they bypass the through model's `save()` and audit
columns. **No production code calls `.add()`/`.set()`** on the Plane M2Ms — every write
goes through explicit `bulk_create` or a serializer that does. If the Rust port exposes
`.add()` semantics, it must decide whether to fill `created_by`; Django does not.

### 4.5 Advisory lock around issue sequence numbers

Only one place takes a lock for sequence allocation:
`plane/db/models/issue.py:184-214` (inside `Issue.save` on create), keyed on
`convert_uuid_to_integer(project.id)` = first 8 bytes of
`sha256(lowercase-hyphenated uuid string)` big-endian signed. `fix_duplicate_sequences`
uses the same key
(`plane/db/management/commands/fix_duplicate_sequences.py:66`). `select_for_update` is
used in only four places, none of them for `sequence_id`:

- `plane/app/views/view/base.py:96,388` — lock an `IssueView` before update.
- `plane/db/models/project.py:185` — lock snoozed `IntakeIssue` rows while retiming.
- `plane/license/api/views/admin.py:221` — lock an `Instance` row.

### 4.6 Raw SQL

- `plane/app/views/page/base.py:73-75` — recursive CTE that sets `archived_at` on a page
  and all descendants. Bypasses `Page.save`, does not touch `updated_at`, and does not
  filter `deleted_at`.
- The two `pg_advisory_xact_lock` calls above.

### 4.7 Management commands

Commands are not HTTP routes but they write the same rows; a port must either keep them
in Django during coexistence or reproduce their hooks. The ones that touch hook-bearing
models: `fix_duplicate_sequences` (Issue/IssueSequence `bulk_update`),
`copy_issue_comment_to_description` (`Description.bulk_create`,
`IssueComment.bulk_update`), `create_dummy_data` / `create_project_member` (bulk
memberships), `update_deleted_workspace_slug` (`workspace.save(update_fields=["slug"])`),
`register_instance`/`configure_instance` (license).

### 4.8 Middleware / request-time writes

- `APIToken.last_used` (`plane/app/middleware/api_authentication.py:42`).
- `APIActivityLog` rows via the `process_logs` Celery task
  (`plane/bgtasks/logger_task.py:25`), enqueued by
  `plane/middleware/logger.py` for `/api/v1/` requests.
- Session rows: written by Django's session backend on login/logout/`cycle_key`
  (`SessionStore.save`), not by a model hook.

---

## 5. Hard delete and FK cascades

### 5.1 The scheduled job

- Task: `plane.bgtasks.deletion_task.hard_delete` — `plane/bgtasks/deletion_task.py:113-193`.
- Schedule: `plane/celery.py:61-64`, **daily at 00:00 UTC**
  (`crontab(hour=0, minute=0)`, beat key `check-every-day-to-delete-hard-delete`).
- Threshold: `settings.HARD_DELETE_AFTER_DAYS`, `plane/settings/common.py:472`,
  env `HARD_DELETE_AFTER_DAYS`, default **60**. Predicate is
  `deleted_at__lt = timezone.now() - timedelta(days=days)` (strictly older than N days;
  `deleted_at IS NULL` never matches).
- No `autoretry_for`; the task is not transactional across the whole run. Each `.delete()`
  is its own transaction, so a failure leaves earlier deletes committed and the rest
  unprocessed.

Order and targets (each line is
`Model.all_objects.filter(deleted_at__lt=cutoff).delete()`; `all_objects` is a plain
`Manager`, so this is a **real DELETE**, not a soft delete):

1. `Workspace` (`:138`)
2. `Project` (`:141`)
3. `Cycle` (`:144`)
4. `Module` (`:147`)
5. `Issue` (`:150`)
6. `Page` (`:153`)
7. `IssueView` (`:156`)
8. `Label` (`:159`)
9. `State` (`:162`)
10. `IssueActivity` (`:164`)
11. `IssueComment` (`:166`)
12. `IssueLink` (`:168`)
13. `IssueReaction` (`:170`)
14. `UserFavorite` (`:172`)
15. `ModuleIssue` (`:174`)
16. `CycleIssue` (`:176`)
17. `Estimate` (`:178`)
18. `EstimatePoint` (`:180`)
19. Then (`:184-191`) for **every** registered model with a `deleted_at` attribute:
    `model.all_objects.filter(deleted_at__lt=cutoff).delete()` — this covers everything
    not named above (workspace members/invites, project members/invites, states, labels,
    pages, comments, reactions, subscriptions, notifications, API tokens, license models,
    …). Models without `deleted_at` (User, Profile, Account, Session,
    django_celery_beat) are skipped.

Because step 1 hard-deletes a Workspace with Django's cascade, **everything belonging to
that workspace is deleted regardless of its own `deleted_at`**: projects, issues,
comments, pages, cycles, modules, states, labels, members, assets, notifications,
activities, … This is the "cascade the same way" requirement. The same applies to an old
Project (step 2): all project-scoped rows go, including rows created or updated recently.

### 5.2 Django emulates cascades in Python; Postgres has no `ON DELETE`

Django's schema editor emits a plain FK:

```sql
ALTER TABLE "<table>" ADD CONSTRAINT "<fk_name>" FOREIGN KEY ("<column>")
REFERENCES "<target_table>" ("<target_column>") DEFERRABLE INITIALLY DEFERRED
```

`apps/api/.venv/.../django/db/backends/base/schema.py:121-124` (`sql_create_fk`),
`:1715-1740` (`_create_fk_sql`) and
`.../django/db/backends/postgresql/operations.py:150-151`
(`deferrable_sql() -> " DEFERRABLE INITIALLY DEFERRED"`). There is **no `ON DELETE`
clause**. The `on_delete=` argument in the models/migrations is Python-only.

Evidence from the migrations: `plane/db/migrations/0001_initial.py:2005` declares
`IssueActivity.issue` as a `ForeignKey(..., related_name="issue_activity")` and
`plane/db/migrations/0108_alter_issueactivity_issue_comment.py:17,22` changes the Python
`on_delete` to `DO_NOTHING`; neither operation adds a database `ON DELETE` action. Every
other FK follows the same pattern (`on_delete=django.db.models.deletion.CASCADE` etc. in
`0001_initial.py` throughout). So:

- A bare `DELETE FROM issues WHERE ...` in Rust **fails** with
  `violates foreign key constraint` if any referencing row exists, even for a
  `CASCADE` relation.
- Within **one transaction**, the deferred constraints make ordering irrelevant: delete
  parents and children in any order and the check happens at `COMMIT`. Django does not use
  this (it deletes child-first from `Collector`), but Rust can.
- `SET_NULL` is **not** applied by Postgres. Rust must issue the `UPDATE ... SET col=NULL`
  itself.
- `DO_NOTHING` is not applied by anyone: if rows reference the deleted row, the
  transaction fails at commit.
- `PROTECT`/`RESTRICT`/`SET_DEFAULT` do not occur in this schema (verified: only CASCADE,
  SET_NULL and DO_NOTHING are used).

Django's `Collector` (`django/db/models/deletion.py`) semantics, which Rust must
reimplement for hard deletes:

1. Collect the objects and every related object reachable through relations whose
   `on_delete` is not `DO_NOTHING`; recurse through CASCADE.
2. `SET_NULL` children are updated to NULL (`QuerySet.update`, no hooks).
3. `CASCADE` children are deleted (their own related rows are collected first).
4. `DO_NOTHING` is skipped entirely.
5. `PROTECT`/`RESTRICT` would raise; none here.
6. Related-object queries use `_base_manager` (a plain `Manager`), so they see
   **soft-deleted rows too**.
7. `pre_delete`/`post_delete` are sent per object (no receivers in Plane).

### 5.3 Every non-CASCADE ForeignKey (206 columns)

All `created_by`/`updated_by` columns are SET_NULL by construction (`UserAuditModel`).
Every other non-CASCADE FK is listed explicitly. There are **no** PROTECT, RESTRICT or
SET_DEFAULT FKs.

| table | non-CASCADE columns |
| --- | --- |
| `analytic_views` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `api_activity_logs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `api_tokens` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `changelogs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `comment_reactions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `cycle_issues` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `cycle_user_properties` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `cycles` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `deploy_boards` | `created_by`, `updated_by` → `users` **SET_NULL**; `intake` → `intakes` **SET_NULL** |
| `description_versions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `descriptions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `device_sessions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `devices` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `draft_issue_assignees` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `draft_issue_cycles` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `draft_issue_labels` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `draft_issue_modules` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `draft_issues` | `created_by`, `updated_by` → `users` **SET_NULL**; `estimate_point` → `estimate_points` **SET_NULL**; `type` → `issue_types` **SET_NULL** |
| `email_notification_logs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `estimate_points` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `estimates` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `exporters` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `file_assets` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `github_comment_syncs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `github_issue_syncs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `github_repositories` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `github_repository_syncs` | `created_by`, `updated_by` → `users` **SET_NULL**; `label` → `labels` **SET_NULL** |
| `importers` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `instance_admins` | `created_by`, `updated_by` → `users` **SET_NULL**; `user` → `users` **SET_NULL** |
| `instance_configurations` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `instances` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `intake_issues` | `created_by`, `updated_by` → `users` **SET_NULL**; `duplicate_to` → `issues` **SET_NULL** |
| `intakes` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `integrations` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_activities` | `created_by`, `updated_by` → `users` **SET_NULL**; `issue` → `issues` **DO_NOTHING**; `issue_comment` → `issue_comments` **DO_NOTHING**; `actor` → `users` **SET_NULL** |
| `issue_assignees` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_attachments` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_blockers` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_comments` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_description_versions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_labels` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_links` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_mentions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_reactions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_relations` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_sequences` | `created_by`, `updated_by` → `users` **SET_NULL**; `issue` → `issues` **SET_NULL** |
| `issue_subscribers` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_types` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_versions` | `created_by`, `updated_by` → `users` **SET_NULL**; `activity` → `issue_activities` **SET_NULL** |
| `issue_views` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issue_votes` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `issues` | `created_by`, `updated_by` → `users` **SET_NULL**; `estimate_point` → `estimate_points` **SET_NULL**; `type` → `issue_types` **SET_NULL** |
| `labels` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `module_issues` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `module_links` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `module_members` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `module_user_properties` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `modules` | `created_by`, `updated_by` → `users` **SET_NULL**; `lead` → `users` **SET_NULL** |
| `notifications` | `created_by`, `updated_by` → `users` **SET_NULL**; `triggered_by` → `users` **SET_NULL** |
| `page_labels` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `page_logs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `page_versions` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `pages` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_deploy_boards` | `created_by`, `updated_by` → `users` **SET_NULL**; `intake` → `intakes` **SET_NULL** |
| `project_identifiers` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_issue_types` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_member_invites` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_members` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_pages` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_public_members` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_user_properties` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `project_webhooks` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `projects` | `created_by`, `updated_by` → `users` **SET_NULL**; `cover_image_asset` → `file_assets` **SET_NULL**; `estimate` → `estimates` **SET_NULL**; `default_state` → `states` **SET_NULL** |
| `slack_project_syncs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `social_login_connections` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `states` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `stickies` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `teams` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `user_favorites` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `user_notification_preferences` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `user_recent_visits` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `users` | `avatar_asset` → `file_assets` **SET_NULL**; `cover_image_asset` → `file_assets` **SET_NULL** |
| `webhook_logs` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `webhooks` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_home_preferences` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_integrations` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_member_invites` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_members` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_themes` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_user_links` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_user_preferences` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspace_user_properties` | `created_by`, `updated_by` → `users` **SET_NULL** |
| `workspaces` | `created_by`, `updated_by` → `users` **SET_NULL**; `logo_asset` → `file_assets` **SET_NULL** |

### 5.4 What Rust must delete/null, by parent

When Rust hard-deletes a row, it must reproduce Django's collector. The CASCADE children
of the important parents (full set from the model introspection; a child that also has a
`workspace_id` is reachable from both the workspace and the project):

| parent | CASCADE children (delete) | SET_NULL children (update to NULL) |
| --- | --- | --- |
| `workspaces` | **every** table with a CASCADE `workspace_id` FK — the 81 tables listed in §5.4.1 | none directly (the audit/`logo_asset` SET_NULLs are on the deleted rows' own FKs, not children) |
| `projects` | **every** table with a CASCADE `project_id` FK — the 61 tables listed in §5.4.2 (a subset of the workspace list; `project` is never set without `workspace`) | none |
| `issues` | `cycle_issues`, `draft_issues(parent)`, `file_assets`, `github_issue_syncs`, `intake_issues`, `issue_assignees`, `issue_attachments`, `issue_blockers(block, blocked_by)`, `issue_comments`, `issue_description_versions`, `issue_labels`, `issue_links`, `issue_mentions`, `issue_reactions`, `issue_relations(issue, related_issue)`, `issue_subscribers`, `issue_versions`, `issue_votes`, `issues(parent)`, `module_issues` | `issue_sequences.issue_id`, `intake_issues.duplicate_to_id`; **`issue_activities.issue_id` is DO_NOTHING** |
| `issue_comments` | `comment_reactions`, `file_assets(comment)`, `github_comment_syncs`, `issue_comments(parent)` | **`issue_activities.issue_comment_id` is DO_NOTHING** |
| `pages` | `file_assets(page)`, `page_labels`, `page_logs`, `page_versions`, `pages(parent)`, `project_pages` | none |
| `modules` | `draft_issue_modules`, `module_issues`, `module_links`, `module_members`, `module_user_properties` | none |
| `cycles` | `cycle_issues`, `cycle_user_properties`, `draft_issue_cycles` | none |
| `labels` | `draft_issue_labels`, `issue_labels`, `labels(parent)`, `page_labels` | `github_repository_syncs.label_id` |
| `states` | `draft_issues(state)`, `issues(state)` | `projects.default_state_id` |
| `estimates` | `estimate_points` | `projects.estimate_id` |
| `estimate_points` | — | `draft_issues.estimate_point_id`, `issues.estimate_point_id` |
| `intakes` | `intake_issues` | `deploy_boards.intake_id`, `project_deploy_boards.intake_id` |
| `descriptions` | `description_versions`, `issue_comments` (the comment's `description` FK is on the comment) | none |
| `file_assets` | — | `projects.cover_image_asset_id`, `users.avatar_asset_id`, `users.cover_image_asset_id`, `workspaces.logo_asset_id` |
| `issue_activities` | — | `issue_versions.activity_id` |
| `issue_types` | `project_issue_types` | `draft_issues.type_id`, `issues.type_id` |
| `integrations` | `workspace_integrations` | — |
| `workspace_integrations` | `github_repository_syncs`, `slack_project_syncs` | — |
| `github_repositories` | `github_repository_syncs(repository)` | — |
| `github_repository_syncs` | `github_issue_syncs` | — |
| `github_issue_syncs` | `github_comment_syncs` | — |
| `instances` | `instance_admins` | — |
| `webhooks` | `project_webhooks` | — |
| `user_favorites` | `user_favorites(parent)` | — |
| `devices` | `device_sessions` | — |
| `sessions` | `device_sessions` | — |
| `api_tokens` | `importers(token)`, `workspace_integrations(api_token)` | — |
| `users` | `accounts`, `api_tokens`, `comment_reactions(actor)`, `cycle_user_properties(user)`, `cycles(owned_by)`, `devices`, `draft_issue_assignees(assignee)`, `email_notification_logs(receiver, triggered_by)`, `exporters(initiated_by)`, `file_assets(user)`, `github_repository_syncs(actor)`, `importers(initiated_by)`, `issue_assignees(assignee)`, `issue_comments(actor)`, `issue_description_versions(owned_by)`, `issue_mentions(mention)`, `issue_reactions(actor)`, `issue_subscribers(subscriber)`, `issue_versions(owned_by)`, `issue_views(owned_by)`, `issue_votes(actor)`, `module_members(member)`, `module_user_properties(user)`, `notifications(receiver)`, `page_versions(owned_by)`, `pages(owned_by)`, `profiles`, `project_members(member)`, `project_public_members(member)`, `project_user_properties(user)`, `projects(default_assignee, project_lead)`, `social_login_connections`, `stickies(owner)`, `user_favorites`, `user_notification_preferences`, `user_recent_visits`, `workspace_home_preferences`, `workspace_integrations(actor)`, `workspace_members(member)`, `workspace_themes(actor)`, `workspace_user_links(owner)`, `workspace_user_preferences`, `workspace_user_properties`, `workspaces(owner)` | every `created_by`/`updated_by` column, plus `instance_admins(user)` |

`descriptions` ↔ `issue_comments`: the FK lives on `issue_comments.description_id`, so
deleting a comment does **not** delete its description (or vice versa: deleting a
description deletes the comment via the CASCADE reverse). Hard-deleting comments leaves
orphan `descriptions` rows, which the final `hard_delete` loop only removes if they were
soft-deleted themselves (they usually are not). Port as-is unless the owner decides
otherwise.

#### 5.4.1 Every table that cascades from `workspaces` (81)

```
analytic_views, api_tokens, comment_reactions, cycle_issues, cycle_user_properties,
cycles, deploy_boards, description_versions, descriptions, draft_issue_assignees,
draft_issue_cycles, draft_issue_labels, draft_issue_modules, draft_issues,
estimate_points, estimates, exporters, file_assets, github_comment_syncs,
github_issue_syncs, github_repositories, github_repository_syncs, importers,
intake_issues, intakes, issue_activities, issue_assignees, issue_attachments,
issue_blockers, issue_comments, issue_description_versions, issue_labels, issue_links,
issue_mentions, issue_reactions, issue_relations, issue_sequences, issue_subscribers,
issue_types, issue_versions, issue_views, issue_votes, issues, labels, module_issues,
module_links, module_members, module_user_properties, modules, notifications,
page_labels, page_logs, page_versions, pages, project_deploy_boards,
project_identifiers, project_issue_types, project_member_invites, project_members,
project_pages, project_public_members, project_user_properties, project_webhooks,
projects, slack_project_syncs, states, stickies, teams, user_favorites,
user_notification_preferences, user_recent_visits, webhook_logs, webhooks,
workspace_home_preferences, workspace_integrations, workspace_member_invites,
workspace_members, workspace_themes, workspace_user_links, workspace_user_preferences,
workspace_user_properties
```

#### 5.4.2 Every table that cascades from `projects` (61)

```
comment_reactions, cycle_issues, cycle_user_properties, cycles, deploy_boards,
description_versions, descriptions, draft_issue_assignees, draft_issue_cycles,
draft_issue_labels, draft_issue_modules, draft_issues, estimate_points, estimates,
file_assets, github_comment_syncs, github_issue_syncs, github_repositories,
github_repository_syncs, importers, intake_issues, intakes, issue_activities,
issue_assignees, issue_attachments, issue_blockers, issue_comments,
issue_description_versions, issue_labels, issue_links, issue_mentions, issue_reactions,
issue_relations, issue_sequences, issue_subscribers, issue_versions, issue_views,
issue_votes, issues, labels, module_issues, module_links, module_members,
module_user_properties, modules, notifications, project_deploy_boards,
project_identifiers, project_issue_types, project_member_invites, project_members,
project_pages, project_public_members, project_user_properties, project_webhooks,
slack_project_syncs, states, user_favorites, user_notification_preferences,
user_recent_visits, workspace_user_links
```

Tables that cascade from a workspace but have no direct `project_id`
(`workspace only`): `analytic_views`, `api_tokens`, `exporters`, `issue_types`,
`page_labels`, `page_logs`, `page_versions`, `pages`, `projects`, `stickies`, `teams`,
`webhook_logs`, `webhooks`, `workspace_home_preferences`, `workspace_integrations`,
`workspace_member_invites`, `workspace_members`, `workspace_themes`,
`workspace_user_preferences`, `workspace_user_properties`. A hard delete of a project
does **not** touch these (except `projects` itself); a hard delete of the workspace does.

### 5.5 Suspected upstream bug: `DO_NOTHING` vs `hard_delete`

`issue_activities.issue_id` and `issue_activities.issue_comment_id` are `DO_NOTHING`
(`plane/db/models/issue.py:416,426`), and the database still has plain FK constraints on
both columns (migration `0001_initial.py:2005`; `0108` only changed the Python
`on_delete`). `soft_delete_related_objects` skips `DO_NOTHING`, so soft-deleting an issue
leaves its activity rows live. `hard_delete` then deletes `issues` (step 5) **before**
`issue_activities` (step 10). Django's collector does not touch the activity rows, so the
`DELETE FROM issues` should fail at commit with a foreign-key violation as soon as any
non-deleted activity references a soft-deleted issue (which the "deleted the issue"
activity row guarantees). The task has no retry and is acked on receipt, so the whole
nightly delete would be lost until the next night.

This was not exercised against a database in this session (no DB connection was made);
it follows from Django's `Collector` (`deletion.py:315` skips `DO_NOTHING`) and the
migration/SQL evidence above. **Verify before porting the job.** Rust should either
(a) reproduce the failure (not useful), or (b) delete `issue_activities` by `issue_id`/
`issue_comment_id` first, inside the same transaction, and record the deviation in the
ledger. The same applies to any user-initiated hard delete of an issue or comment.

### 5.6 Other hard-delete jobs

Not part of `hard_delete`, but they delete rows with `all_objects` (real DELETE, no
hooks) and are scheduled (`plane/celery.py:69-100`):

- `delete_api_logs` — `APIActivityLog`, `created_at <= now - API_ACTIVITY_LOG_RETENTION_DAYS`
  (default 14, `plane/settings/common.py:493`).
- `delete_email_notification_logs` — `EmailNotificationLog`, `sent_at <= now - EMAIL_LOG_RETENTION_DAYS`
  (default 7, `:500`).
- `delete_page_versions` — `PageVersion` beyond the newest 20 per page.
- `delete_issue_description_versions` — `IssueDescriptionVersion` beyond the newest 20 per
  issue.
- `delete_webhook_logs` — `WebhookLog`, `created_at <= now - WEBHOOK_LOG_RETENTION_DAYS`
  (default 14, `:497`).

All use `model.all_objects.filter(...)` and `process_cleanup_task` batches of 500
(`plane/bgtasks/cleanup_task.py:34-81,161-208`), with failures caught per batch. None of
these models has CASCADE children, so a plain `DELETE` is enough.

---

## 6. Hook checklist

`Status` is `not-ported` for every row. `Rust function` is the proposed home in
`plane-db`; names are suggestions, the behaviour is the contract. "Trigger" is
create/update/delete/signal.

| ID | Model(s) | Trigger | Behaviour | Proposed Rust function | Status |
| --- | --- | --- | --- | --- | --- |
| H01 | every `BaseModel` subclass (90 concrete models) | create/update | set `created_by`/`updated_by` from the acting user: create → `created_by=user, updated_by=NULL`; update → `updated_by=user`; no user → both NULL; `created_by_id=` overrides; `disable_auto_set_user` skips | `plane_db::hooks::audit::stamp(model, user, mode, disable)` | not-ported |
| H02 | `Issue`, `IssueComment`, `Project`, `IntakeIssue` | create/update | capture tracked "before" values; expose `changed` to the model hook; reset after save (`_changes_on_save`) | `plane_db::hooks::change_tracker::{load_before, changed_fields}` | not-ported |
| H03 | every `SoftDeleteModel` (92 models) | delete | soft: set `deleted_at=now`, full save (runs the model hook + H01), enqueue `soft_delete_related_objects`; hard (`soft=False`): Django-style delete | `plane_db::hooks::soft_delete::delete_instance(tx, id, soft, user)` | not-ported |
| H04 | every `SoftDeletionQuerySet` (`Model.objects.filter(...).delete()`) | delete | `UPDATE ... SET deleted_at=now`; no save hook, no `updated_at`, no cascade task, returns row count | `plane_db::hooks::soft_delete::delete_where(sql_predicate)` | not-ported |
| H05 | `Workspace` | delete | after the soft delete, append `__{epoch_seconds}` to `slug` and save only `slug` | `plane_db::hooks::workspace::after_soft_delete` | not-ported |
| H06 | `WorkspaceBaseModel` subclasses | create/update | if `project_id` is set, set `workspace_id = project.workspace_id` | `plane_db::hooks::scope::workspace_from_project` | not-ported |
| H07 | `ProjectBaseModel` subclasses | create/update | always set `workspace_id = project.workspace_id` | `plane_db::hooks::scope::workspace_from_project` | not-ported |
| H08 | `Project` | create | uppercase/strip `identifier`; if timezone not supplied, copy `workspace.timezone`; `__init__` flag | `plane_db::hooks::project::before_insert` | not-ported |
| H09 | `Project` | update | uppercase/strip `identifier`; if the persisted timezone changed, run H09a in the same transaction | `plane_db::hooks::project::before_update` | not-ported |
| H09a | `IntakeIssue` | (called by H09) | lock snoozed rows, move `snoozed_till` to the same calendar day midnight in the new timezone, `bulk_update` | `plane_db::hooks::project::retime_intake_snoozes` | not-ported |
| H10 | `Issue` | create | default state if none (H10a), `completed_at` sync (H10b), advisory lock on project, `sequence_id = max(IssueSequence.sequence)+1`, strip HTML, `sort_order = max(Issue.sort_order same project+state)+10000` | `plane_db::hooks::issue::before_insert` | not-ported |
| H10a | `Issue` | create | if `state_id IS NULL`: first `default` non-triage state of the project, else first non-triage state | `plane_db::hooks::issue::ensure_default_state` | not-ported |
| H10b | `Issue` | create/update | if state set and (creating or state changed): `completed_at = now` when group=completed else NULL; add `completed_at` to `update_fields` | `plane_db::hooks::issue::sync_completed_at` | not-ported |
| H11 | `Issue` | create | insert the `issue_sequences` row with `sequence = issue.sequence_id`, same transaction/lock | `plane_db::hooks::issue::after_insert` | not-ported |
| H12 | `Issue` | update | strip HTML into `description_stripped` | `plane_db::hooks::issue::before_update` | not-ported |
| H13 | `Issue` | delete | inherited soft delete runs H10a/H10b/H12 before writing `deleted_at` | (covered by H03 + H10a/H10b/H12) | not-ported |
| H14 | `IssueComment` | create | strip HTML (`""` stays `""`); create the `Description` from the in-memory audit ids (NULL on DRF create), set `description_id`, second save of `description_id` | `plane_db::hooks::issue_comment::before_insert` / `after_insert` | not-ported |
| H15 | `IssueComment` | update | strip HTML; for each changed tracked comment field, column-update the matching `Description` field + `updated_by_id`/`updated_at` | `plane_db::hooks::issue_comment::before_update` | not-ported |
| H16 | `IntakeIssue` | create/update | if `snoozed_till` set and new/changed: floor to midnight in the project timezone | `plane_db::hooks::intake_issue::before_save` | not-ported |
| H17 | `DraftIssue` | create | default state if none (same query as H10a); `completed_at` from state group; strip HTML; `sort_order = max(draft same project+state)+10000` | `plane_db::hooks::draft_issue::before_insert` | not-ported |
| H18 | `DraftIssue` | update | recompute `completed_at` from the state group **every save**; strip HTML | `plane_db::hooks::draft_issue::before_update` | not-ported |
| H19 | `State` | create/update | `slug = slugify(name)` every save; on create `sequence = max(non-triage state in project)+15000` or default 65535 | `plane_db::hooks::state::before_save` | not-ported |
| H20 | `Label` | create | `sort_order = max(label same project)+10000` or 65535 | `plane_db::hooks::label::before_insert` | not-ported |
| H21 | `Module` | create | `sort_order = min(module same project)-10000` or 65535 | `plane_db::hooks::module::before_insert` | not-ported |
| H22 | `Cycle` | create | `sort_order = min(cycle same project)-10000` or 65535 | `plane_db::hooks::cycle::before_insert` | not-ported |
| H23 | `Sticky` | create/update | strip HTML; on create `sort_order = max(sticky same workspace)+10000` or 65535 | `plane_db::hooks::sticky::before_save` | not-ported |
| H24 | `IssueView` | create/update | `query = issue_filters(filters)` or `{}`; on create `sort_order = max(view same project or workspace-level)+10000` or 65535 | `plane_db::hooks::issue_view::before_save` | not-ported |
| H25 | `UserFavorite` | create | `sequence = max(favorite same workspace)+10000` or 65535 | `plane_db::hooks::user_favorite::before_insert` | not-ported |
| H26 | `Page` | create/update | strip HTML into `description_stripped` (plane stripper, `None` for empty) | `plane_db::hooks::page::before_save` | not-ported |
| H27 | `PageVersion` | create/update | strip HTML into `description_stripped` (plane stripper) | `plane_db::hooks::page_version::before_save` | not-ported |
| H28 | `Description` | create/update | `description_stripped = django.utils.html.strip_tags` (entities kept) | `plane_db::hooks::description::before_save` | not-ported |
| H29 | `DescriptionVersion` | create/update | same as H28 | `plane_db::hooks::description_version::before_save` | not-ported |
| H30 | `User` | create/update | lowercase/strip email; rotate `token` when `token_updated_at` set; `display_name` from email; `is_superuser` → `is_staff` | `plane_db::hooks::user::before_save` | not-ported |
| H31 | `User` | insert (signal) | create the `UserNotificationPreference` row when `not is_bot` | `plane_db::hooks::user::after_insert` | not-ported |
| H32 | `ProjectMember` | create | create the `ProjectUserProperty` at `min-10000`/65535 when `member` is set | `plane_db::hooks::project_member::before_insert` | not-ported |
| H33 | `IssueVersion` (classmethod) | create | snapshot the issue, its cycle/assignees/labels/modules, `last_saved_at`, `owned_by` | `plane_db::hooks::issue_version::log(issue_id, user)` | not-ported |
| H34 | `IssueDescriptionVersion` (classmethod) | create | snapshot the issue's description + audit ids, `last_saved_at`, `owned_by` | `plane_db::hooks::issue_description_version::log(issue_id, user)` | not-ported |
| H35 | any soft-deletable model | delete | `soft_delete_related_objects`: reverse-relation cascade (SET_NULL / soft-delete recursion), skip DO_NOTHING, final self delete | `plane_db::hooks::soft_delete::cascade_related(tx, app, model, pk)` | not-ported |
| H36 | `Workspace`, `Project`, `Cycle`, `Module`, `Issue`, `Page`, `IssueView`, `Label`, `State`, `IssueActivity`, `IssueComment`, `IssueLink`, `IssueReaction`, `UserFavorite`, `ModuleIssue`, `CycleIssue`, `Estimate`, `EstimatePoint`, all `deleted_at` models | scheduled delete | nightly `hard_delete` with `HARD_DELETE_AFTER_DAYS` (default 60), Django-style cascade per §5 | `plane_jobs::tasks::deletion::hard_delete` | not-ported |

---

## Appendix A — completeness grep

Commands used (from `apps/api`):

```
grep -rn "def save(\|def delete(\|@receiver(\|\.connect(" plane --include="*.py" \
  | grep -v "/tests/" | grep -v "/migrations/"
```

### A.1 `def save(` — 22 model occurrences + 1 serializer

| file:line | symbol | maps to |
| --- | --- | --- |
| `plane/db/mixins.py:196` | `ChangeTrackerMixin.save` | H02 |
| `plane/db/models/base.py:23` | `BaseModel.save` | H01 |
| `plane/db/models/cycle.py:88` | `Cycle.save` | H22 |
| `plane/db/models/description.py:22` | `Description.save` | H28 |
| `plane/db/models/description.py:49` | `DescriptionVersion.save` | H29 |
| `plane/db/models/draft.py:84` | `DraftIssue.save` | H17, H18 |
| `plane/db/models/favorite.py:52` | `UserFavorite.save` | H25 |
| `plane/db/models/intake.py:100` | `IntakeIssue.save` | H16 |
| `plane/db/models/issue.py:180` | `Issue.save` | H10, H10a, H10b, H11, H12 |
| `plane/db/models/issue.py:481` | `IssueComment.save` | H14, H15 |
| `plane/db/models/label.py:46` | `Label.save` | H20 |
| `plane/db/models/module.py:115` | `Module.save` | H21 |
| `plane/db/models/page.py:70` | `Page.save` | H26 |
| `plane/db/models/page.py:175` | `PageVersion.save` | H27 |
| `plane/db/models/project.py:193` | `Project.save` | H08, H09, H09a |
| `plane/db/models/project.py:237` | `ProjectBaseModel.save` | H07 |
| `plane/db/models/project.py:276` | `ProjectMember.save` | H32 |
| `plane/db/models/state.py:117` | `State.save` | H19 |
| `plane/db/models/sticky.py:38` | `Sticky.save` | H23 |
| `plane/db/models/user.py:169` | `User.save` | H30 |
| `plane/db/models/view.py:79` | `IssueView.save` | H24 |
| `plane/db/models/workspace.py:192` | `WorkspaceBaseModel.save` | H06 |
| `plane/api/serializers/state.py:27` | `StateSerializer.save` | **not a model hook** — serializer: saves the state then `State.objects.filter(project).exclude(pk).update(default=False)` (see §4.2) |

Not matching `def save(` but part of the same hook: `Issue._ensure_default_state`
(`issue.py:228`) / `_sync_completed_at` (`:240`), `Project._retime_intake_snoozes`
(`project.py:169`), `IssueVersion.log_issue_version` (`:736`),
`IssueDescriptionVersion.log_issue_description_version` (`:800`),
`SoftDeleteModel.delete` (`mixins.py:72`), `SoftDeletionQuerySet.delete`
(`mixins.py:49`), `Workspace.delete` (`workspace.py:156`).

### A.2 `def delete(` — model hooks vs HTTP handlers

Model hooks:

| file:line | symbol | maps to |
| --- | --- | --- |
| `plane/db/mixins.py:49` | `SoftDeletionQuerySet.delete(soft=True)` | H04 |
| `plane/db/mixins.py:72` | `SoftDeleteModel.delete(using, soft, ...)` | H03, H35 |
| `plane/db/models/workspace.py:156` | `Workspace.delete` | H05 |

Every other `def delete(` is a DRF view method that handles an HTTP `DELETE` (or, in two
cases, an unarchive action) — **not** a model hook. They are listed so the grep is
complete; each ultimately exercises H03/H04/H36 through the model it deletes:

| file:line | note |
| --- | --- |
| `plane/api/views/asset.py:233` | user avatar/cover asset (FileAsset flags) |
| `plane/api/views/asset.py:389` | project/issue asset (FileAsset flags) |
| `plane/api/views/cycle.py:617` | deletes a `Cycle` (H03) |
| `plane/api/views/cycle.py:832` | **unarchive** cycle (`archived_at=None`, save) — not a delete |
| `plane/api/views/cycle.py:1150` | deletes a `CycleIssue` (H03) |
| `plane/api/views/estimate.py:129` | deletes an `Estimate` (H03) |
| `plane/api/views/estimate.py:286` | deletes an `EstimatePoint` (H03) |
| `plane/api/views/intake.py:469` | deletes an `IntakeIssue` (H03) |
| `plane/api/views/issue.py:914` | deletes an `Issue` (H03) |
| `plane/api/views/issue.py:1178` | deletes a `Label` (H03) |
| `plane/api/views/issue.py:1433` | deletes an `IssueLink` (H03) |
| `plane/api/views/issue.py:1791` | deletes an `IssueComment` (H03) |
| `plane/api/views/issue.py:2202` | deletes an issue attachment (`FileAsset` flags) |
| `plane/api/views/member.py:231` | deactivates a `ProjectMember` (save) |
| `plane/api/views/module.py:541` | deletes a `Module` (H03) |
| `plane/api/views/module.py:925` | deletes a `ModuleIssue` (H03) |
| `plane/api/views/module.py:1129` | deletes a `ModuleLink` (H03) |
| `plane/api/views/project.py:627` | deletes a `Project` (H03) |
| `plane/api/views/project.py:696` | project membership removal |
| `plane/api/views/state.py:236` | deletes a `State` (H03) |
| `plane/app/views/api.py:51` | deletes an `APIToken` (H03) |
| `plane/app/views/asset/base.py:55` | deletes a `FileAsset` (flags) |
| `plane/app/views/asset/base.py:105` | deletes a `FileAsset` (flags) |
| `plane/app/views/asset/v2.py:233` | deletes a `FileAsset` (flags) |
| `plane/app/views/asset/v2.py:475` | deletes a `FileAsset` (flags) |
| `plane/app/views/asset/v2.py:705` | deletes a `FileAsset` (flags) |
| `plane/app/views/cycle/archive.py:607` | **unarchive** cycle (save) |
| `plane/app/views/issue/attachment.py:72` | deletes a `FileAsset`/legacy attachment (flags + H03) |
| `plane/app/views/issue/attachment.py:181` | deletes a `FileAsset` (flags) |
| `plane/app/views/issue/base.py:777` | `BulkDeleteIssuesEndpoint` — `Issue.issue_objects…delete()` = **H04** (bulk soft, no cascade task) |
| `plane/app/views/module/archive.py:561` | **unarchive** module (save) |
| `plane/app/views/project/base.py:438` | archives/unarchives a project (save) |
| `plane/app/views/project/base.py:458` | deletes a `ProjectIdentifier` (H04) |
| `plane/app/views/user/base.py:436` | deletes an `Account` — hard delete (not soft-deletable) |
| `plane/app/views/webhook/base.py:108` | deletes a `Webhook` (H03) |
| `plane/app/views/workspace/favorite.py:90` | `favorite.delete(soft=False)` — **hard** delete (H03 hard branch) |
| `plane/license/api/views/admin.py:85` | removes an `InstanceAdmin` (H04) |
| `plane/license/api/views/configuration.py:66` | clears an `InstanceConfiguration` value (save) |
| `plane/space/views/asset.py:205` | deletes/binds a `FileAsset` (flags) |

### A.3 `@receiver(` — 1

| file:line | symbol | maps to |
| --- | --- | --- |
| `plane/db/models/user.py:298` | `create_user_notification` (`post_save`, `sender=User`) | H31 / S01 |

No other `@receiver` under `apps/api/plane/` (verified).

### A.4 `.connect(` — 0 occurrences

`grep -rn "\.connect(" plane --include="*.py"` (excluding tests/migrations) returns
**no** matches: the only signal connections in the codebase are decorators without
parentheses.

| file:line | symbol | maps to |
| --- | --- | --- |
| `plane/celery.py:105` | `@after_setup_logger.connect` (Celery signal) | not a model hook |
| `plane/celery.py:113` | `@after_setup_task_logger.connect` (Celery signal) | not a model hook |

These are Celery's logger-setup signals; they do not touch the database. No Django signal
is connected with `.connect()` anywhere — the single Django receiver uses `@receiver`
(§A.3).

### A.5 Supporting greps

```
grep -rn "post_save\|pre_save\|post_delete\|pre_delete\|m2m_changed\|Signal(\|receiver(\|dispatch_uid" \
  plane --include="*.py" | grep -v "/tests/" | grep -v "/migrations/"
→ only plane/db/models/user.py:15 (import) and :298 (the receiver above)
```

```
grep -rn "def clean\|full_clean\|validate_unique" plane --include="*.py" \
  | grep -v "/tests/" | grep -v "/migrations/"
→ no matches. No model defines clean(); Django validation is never run on save().
```

```
grep -rn "bulk_create\|bulk_update\|select_for_update" plane --include="*.py" \
  | grep -v "/tests/" | grep -v "/migrations/"
→ 105 production lines: 101 `bulk_create`/`bulk_update` tabulated in §4.1, 4 `select_for_update` in §4.5
```

---

## Appendix B — counts

Introspection (no database; `django.setup()` only):

```bash
cd apps/api
DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=x \
DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \
.venv/bin/python -c "
import django; django.setup()
from django.apps import apps
print(len([m for m in apps.get_models() if not m._meta.abstract]))
"
```

| measure | count |
| --- | --- |
| concrete models in the whole project (`apps.get_models()`, non-abstract) | **106** |
| … `db` app | **92** |
| … `license` app | **4** (`Instance`, `InstanceAdmin`, `InstanceConfiguration`, `ChangeLog`) |
| … third-party (`auth` 2, `contenttypes` 1, `django_celery_beat` 6, `sessions` 1) | **10** |
| concrete models documented in §2 | **96** (= 92 + 4) |
| abstract model bases documented | 7 (`TimeAuditModel`, `UserAuditModel`, `SoftDeleteModel`, `AuditModel`, `BaseModel` in §1; `ProjectBaseModel`, `WorkspaceBaseModel` in §2) |
| models with a `save()` override | **18** concrete models (`User`, `Project`, `ProjectMember`, `Issue`, `IssueComment`, `Page`, `PageVersion`, `Description`, `DescriptionVersion`, `Sticky`, `State`, `Label`, `Module`, `Cycle`, `IntakeIssue`, `DraftIssue`, `IssueView`, `UserFavorite`) + 3 abstract bases with `save()` (`BaseModel`, `ProjectBaseModel`, `WorkspaceBaseModel`) + the `ChangeTrackerMixin` |
| models with a `delete()` override | **1** (`Workspace`) |
| signals | **1** receiver |
| soft-deletable models | **92** (96 − 4) |
| non-CASCADE FK columns | **206** (all SET_NULL except `issue_activities.issue`, `issue_activities.issue_comment` = DO_NOTHING) |

Count reconciliation: `apps.get_models()` returns 106; 10 are third-party;
106 − 10 = 96 Plane models; §2 has a heading for each of the 96 and the introspection
loop in this session printed exactly those 96 `db`/`license` rows. No model class is
defined outside the files listed in §2 (`find apps/api/plane -name "*.py" -path "*model*"`
returns only `plane/db/models/**` and `plane/license/models/**`).

Hook counts: **39** checklist rows — H01–H36 plus the sub-hooks H09a, H10a and H10b.

### B.2 Callable field defaults (complete)

Every model below evaluates the callable **per instance** (`Model.__init__` → `Field.get_default`),
so the value is generated in Python, not in Postgres. Rust must generate the same value at
INSERT time when the caller does not supply the column. Unless noted, a `BaseModel`
subclass also has `id = uuid.uuid4` (a fresh UUID4 per row). `Project`-scoped models also
inherit the base fields listed in §1.

| model | callable default(s) |
| --- | --- |
| `Account` | `id=uuid.uuid4`; `last_connected_at=timezone.now`; `metadata=dict` |
| `AnalyticView` | `id=uuid.uuid4`; `query_dict=dict` |
| `APIActivityLog` | `id=uuid.uuid4` |
| `APIToken` | `id=uuid.uuid4`; `label=generate_label_token()` → `uuid4().hex`; `token=generate_token()` → `"plane_api_" + uuid4().hex` |
| `ChangeLog` | `id=uuid.uuid4`; `tags=list` |
| `CommentReaction` | `id=uuid.uuid4` |
| `Cycle` | `id=uuid.uuid4`; `view_props=dict`; `progress_snapshot=dict`; `logo_props=dict` |
| `CycleIssue` | `id=uuid.uuid4` |
| `CycleUserProperties` | `id=uuid.uuid4`; `filters=cycle.get_default_filters()`; `display_filters=cycle.get_default_display_filters()`; `display_properties=cycle.get_default_display_properties()`; `rich_filters=dict` |
| `DeployBoard` | `id=uuid.uuid4`; `anchor=get_anchor()` → `uuid4().hex`; `view_props=dict` |
| `Description` | `id=uuid.uuid4`; `description_json=dict` |
| `DescriptionVersion` | `id=uuid.uuid4`; `description_json=dict` |
| `Device` / `DeviceSession` | `id=uuid.uuid4` |
| `DraftIssue` | `id=uuid.uuid4`; `description_json=dict` |
| `DraftIssueAssignee` / `DraftIssueLabel` / `DraftIssueModule` / `DraftIssueCycle` | `id=uuid.uuid4` |
| `EmailNotificationLog` | `id=uuid.uuid4` |
| `Estimate` / `EstimatePoint` | `id=uuid.uuid4` |
| `ExporterHistory` | `id=uuid.uuid4`; `token=generate_token()` → `uuid4().hex`; `rich_filters=dict` |
| `FileAsset` | `id=uuid.uuid4`; `attributes=dict`; `storage_metadata=dict` |
| `GithubRepository` | `id=uuid.uuid4`; `config=dict` |
| `GithubRepositorySync` | `id=uuid.uuid4`; `credentials=dict` |
| `GithubIssueSync` / `GithubCommentSync` | `id=uuid.uuid4` |
| `Importer` | `id=uuid.uuid4`; `metadata=dict`; `config=dict`; `data=dict` |
| `Instance` / `InstanceAdmin` / `InstanceConfiguration` | `id=uuid.uuid4` |
| `Intake` | `id=uuid.uuid4`; `view_props=dict`; `logo_props=dict` |
| `IntakeIssue` | `id=uuid.uuid4`; `extra=dict` |
| `Integration` | `id=uuid.uuid4` (own field, not BaseModel); `description=dict`; `metadata=dict` |
| `Issue` | `id=uuid.uuid4`; `description_json=dict` |
| `IssueActivity` | `id=uuid.uuid4`; `attachments=list` |
| `IssueAssignee` / `IssueBlocker` / `IssueLabel` / `IssueSequence` / `IssueSubscriber` / `IssueVote` | `id=uuid.uuid4` |
| `IssueAttachment` | `id=uuid.uuid4`; `attributes=dict` |
| `IssueComment` | `id=uuid.uuid4`; `comment_json=dict`; `attachments=list` |
| `IssueDescriptionVersion` | `id=uuid.uuid4`; `description_json=dict`; `last_saved_at=timezone.now` |
| `IssueLink` | `id=uuid.uuid4`; `metadata=dict` |
| `IssueMention` / `IssueReaction` / `IssueRelation` | `id=uuid.uuid4` |
| `IssueType` | `id=uuid.uuid4`; `logo_props=dict` |
| `IssueVersion` | `id=uuid.uuid4`; `assignees=list`; `labels=list`; `modules=list`; `properties=dict`; `meta=dict`; `last_saved_at=timezone.now` |
| `IssueView` | `id=uuid.uuid4`; `filters=dict`; `display_filters=view.get_default_display_filters()`; `display_properties=view.get_default_display_properties()`; `rich_filters=dict`; `logo_props=dict` |
| `Label` | `id=uuid.uuid4` |
| `Module` | `id=uuid.uuid4`; `view_props=dict`; `logo_props=dict` |
| `ModuleIssue` / `ModuleMember` | `id=uuid.uuid4` |
| `ModuleLink` | `id=uuid.uuid4`; `metadata=dict` |
| `ModuleUserProperties` | `id=uuid.uuid4`; `filters=module.get_default_filters()`; `display_filters=module.get_default_display_filters()`; `display_properties=module.get_default_display_properties()`; `rich_filters=dict` |
| `Notification` | `id=uuid.uuid4` |
| `Page` | `id=uuid.uuid4`; `description_json=dict`; `view_props=get_view_props()` → `{"full_width": False}`; `logo_props=dict` |
| `PageLabel` / `ProjectPage` / `ProjectPublicMember` / `ProjectWebhook` / `ProjectMemberInvite` / `ProjectIssueType` | `id=uuid.uuid4` |
| `PageLog` | `id=uuid.uuid4`; `transaction=uuid.uuid4` (a UUID object) |
| `PageVersion` | `id=uuid.uuid4`; `last_saved_at=timezone.now`; `description_json=dict`; `sub_pages_data=dict` |
| `Profile` | `id=uuid.uuid4` (own field); `theme=dict`; `onboarding_step=get_default_onboarding()`; `mobile_onboarding_step=get_mobile_default_onboarding()`; `goals=dict`; `background_color=get_random_color()`; `product_tour=get_default_product_tour()` |
| `Project` | `id=uuid.uuid4`; `logo_props=dict` |
| `ProjectDeployBoard` | `id=uuid.uuid4`; `anchor=get_anchor()` → `uuid4().hex`; `views=get_default_views()` |
| `ProjectIdentifier` | **no UUID id** (`BigAutoField`); no callable defaults |
| `ProjectMember` | `id=uuid.uuid4`; `view_props=get_default_props()`; `default_props=get_default_props()`; `preferences=get_default_preferences()` |
| `ProjectUserProperty` | `id=uuid.uuid4`; `filters=issue.get_default_filters()`; `display_filters=issue.get_default_display_filters()`; `display_properties=issue.get_default_display_properties()`; `rich_filters=dict`; `preferences=get_default_preferences()` |
| `Session` | **no id** (`session_key` PK); `device_info` default is literal `None`, not callable |
| `SlackProjectSync` | `id=uuid.uuid4`; `data=dict` |
| `SocialLoginConnection` | `id=uuid.uuid4`; `last_login_at=timezone.now`; `last_received_at=timezone.now` |
| `State` | `id=uuid.uuid4` |
| `Sticky` | `id=uuid.uuid4`; `description=dict`; `logo_props=dict` |
| `Team` | `id=uuid.uuid4`; `logo_props=dict` |
| `User` | `id=uuid.uuid4` (own field); `last_active=timezone.now` |
| `UserFavorite` / `UserNotificationPreference` / `UserRecentVisit` | `id=uuid.uuid4` |
| `Webhook` | `id=uuid.uuid4`; `secret_key=generate_token()` → `"plane_wh_" + uuid4().hex` |
| `WebhookLog` | `id=uuid.uuid4` |
| `WorkspaceIntegration` | `id=uuid.uuid4`; `metadata=dict`; `config=dict` |
| `Workspace` | `id=uuid.uuid4`; `background_color=get_random_color()` |
| `WorkspaceHomePreference` | `id=uuid.uuid4`; `config=dict` |
| `WorkspaceMember` | `id=uuid.uuid4`; `view_props=get_default_props()`; `default_props=get_default_props()`; `issue_props=get_issue_props()`; `getting_started_checklist=dict`; `tips=dict`; `explored_features=dict` |
| `WorkspaceMemberInvite` | `id=uuid.uuid4` |
| `WorkspaceTheme` | `id=uuid.uuid4`; `colors=dict` |
| `WorkspaceUserLink` | `id=uuid.uuid4`; `metadata=dict` |
| `WorkspaceUserPreference` | `id=uuid.uuid4` |
| `WorkspaceUserProperties` | `id=uuid.uuid4`; `filters=workspace.get_default_filters()`; `display_filters=workspace.get_default_display_filters()`; `display_properties=workspace.get_default_display_properties()`; `rich_filters=dict` |

`get_random_color()` (`plane/utils/color.py:9-14`) returns `"#" + "".join(random.choices(string.hexdigits, k=6))`;
`string.hexdigits` is `0123456789abcdefABCDEF`, so the output is **mixed case** and Rust
must draw from the same 22-character alphabet (or accept a distribution change, recorded).

`issue.get_default_properties()` (`issue.py:29-44`) and
`notification.get_default_preference()` (`notification.py:72-78`) are defined but **not
used as model defaults** (no field references them); they exist for serializers/callers.

### B.3 Callable `upload_to` and validators (not field defaults, but run on write)

| model.field | callable | behaviour |
| --- | --- | --- |
| `FileAsset.asset` | `asset.get_upload_path` (`asset.py:20`) | `"{workspace_id}/{uuid4().hex}-{sanitize_filename(filename) or uuid4().hex}"`; if no workspace: `"user-{uuid4().hex}-{filename}"` |
| `IssueAttachment.asset` | `issue.get_upload_path` (`issue.py:387`) | `"{workspace_id}/{uuid4().hex}-{sanitize_filename(filename) or uuid4().hex}"` (legacy model) |
| `FileAsset.asset` / `IssueAttachment.asset` | `file_size` (`asset.py:27`, `issue.py:392`) | raises `ValidationError` when `value.size > settings.FILE_SIZE_LIMIT` (env `FILE_SIZE_LIMIT`, default 5 242 880 = 5 MiB) |
| `Webhook.url` | `validate_schema`, `validate_domain` (`webhook.py:21-31`) | scheme must be http/https; host exactly `localhost`/`127.0.0.1` rejected |
| `Workspace.slug` | `slug_validator` (`workspace.py:114`) | rejects `RESTRICTED_WORKSPACE_SLUGS` (`plane/utils/constants.py:5`) |

These run through DRF (model validators are copied into serializer fields,
`rest_framework/utils/field_mapping.py:get_field_kwargs`), **not** on `Model.save()`.

---

## Appendix C — write paths and the hooks they must call

Minimum set per domain (the endpoint list is in `ROUTES.md`; this is the hook call map).
Any path that writes one of these tables also needs H01 unless stated otherwise.

| write path | tables written | hooks that MUST run | hooks that must NOT run (Django does not) |
| --- | --- | --- | --- |
| signup / OAuth first login (`plane/authentication/adapter/base.py:352-374`) | `users`, `profiles` (later), `user_notification_preferences` | H30, H31 | — |
| workspace create (`plane/app/views/workspace/base.py:82-140`) | `workspaces`, `workspace_members` | H01 | — |
| workspace seed (`plane/bgtasks/workspace_seed_task.py`) | projects, members, states, labels, issues, cycles, modules, pages, … | H01 with `created_by_id=bot`, H07/H08/H19/H20/H21/H22/H26/H10 (per `save()` calls); explicit `IssueSequence` create (default sequence 1) | H32 for bulk `ProjectMember`; H19 for bulk-created states if any |
| project create (`plane/app/views/project/base.py:252-292`) | `projects`, `project_members`, `project_user_properties`, `states`, `project_identifiers` | H01, H08, H32 (via ProjectMember.save), H19 if created one-by-one | H19 for the six `DEFAULT_STATES` (`bulk_create` → empty slug); H32 for bulk memberships |
| project update/archive (`plane/app/views/project/base.py`, `plane/api/views/project.py`) | `projects` | H01, H09 (+H09a) | — |
| project delete | `projects` + children | H03, H05 not applicable, H35 | — |
| issue create (`plane/app/serializers/issue.py:209`, `api/serializers/issue.py:199`, `space/serializer/issue.py:311`) | `issues`, `issue_sequences`, `issue_assignees`, `issue_labels` | H01, H02, H10, H10a, H10b, H11, H12 | H01 for the through-row `bulk_create` (audit copied explicitly) |
| issue update | `issues`, through rows | H01, H02, H10b, H12 | — |
| issue delete (`plane/app/views/issue/base.py:720`) | `issues` | H03 (runs H10a/H10b/H12), H35 | — |
| bulk issue delete (`plane/app/views/issue/base.py:786`) | `cycle_issues`, `module_issues`, `issues` | H04 for all three querysets | H03/H35 (no cascade task) |
| comment create/update/delete | `issue_comments`, `descriptions` | H01, H02, H14/H15 | Description hooks via the column update in H15 |
| attachment upload | `file_assets` | H01 | — |
| cycle create/update/delete | `cycles` | H01, H22 (create) | — |
| module create/update/delete | `modules` | H01, H21 (create) | — |
| state create/update/delete | `states` | H01, H19 | H19 for `bulk_create(DEFAULT_STATES)` |
| label create/update/delete | `labels` | H01, H20 (create) | H20 for `Label.objects.bulk_create` |
| view create/update/delete | `issue_views` | H01, H24 | — |
| favourite create/delete | `user_favorites` | H01, H25 | — |
| sticky create/update/delete | `stickies` | H01, H23 | — |
| page create/update/delete | `pages`, `page_versions`, `page_labels`, `project_pages` | H01, H26, H27 | — |
| intake snooze/unsnooze | `intake_issues` | H01, H16 | H16 for the bulk unsnooze |
| intake accept | `issues`, `intake_issues`, `issue_sequences` | H10/H10a/H10b/H11/H12, H01 | — |
| project member add/remove | `project_members`, `project_user_properties` | H01, H32 for per-row creates | H32 for `bulk_create` |
| user settings/profile | `profiles`, `users` | H30 (user) | — |
| API token create | `api_tokens` | H01 | — |
| webhook create/update/delete | `webhooks` | H01 | — |
| export create | `exporters` | H01 | — |
| `last_used` | `api_tokens` | H01 with `update_fields=["last_used"]` (audit not written) | — |

---

*End of document. Regenerate the counts in Appendix B if models are added. When a hook is
ported, change its checklist status and record the Rust function and test name in
`PROGRESS.md`.*
