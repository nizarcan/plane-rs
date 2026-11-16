#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Build the rich, deterministic read fixture (`plane_fix_tpl`).

The template database produced by `dev/bootstrap.sh` has the migrated schema,
one instance row and 36 instance-configuration rows, and nothing else. A GET
route is only proven by the differential harness if Django returns real,
varied data, so this script fills a clone of that template with a realistic
workspace: every entity type, every role, every status, every soft-delete
state, and timestamps spread over the past year.

What makes it faithful
----------------------
Rows are created through the Django ORM, so every `save()` hook runs exactly as
it does in production:

* `Issue.save` allocates `sequence_id` under the project advisory lock, writes
  the `IssueSequence` row, computes `description_stripped` and `sort_order`,
  and defaults the state when none is given.
* `IssueComment.save` writes its `Description` row.
* `ProjectMember.save` writes the `ProjectUserProperty` row.
* `State.save` slugifies, `Label.save`/`Cycle.save`/`Module.save`/
  `Sticky.save`/`IssueView.save`/`UserFavorite.save` assign sort orders.
* `User.save` normalises the email; the `post_save` signal on `User` creates
  the `UserNotificationPreference` row (the same signal that runs on signup).

Two related rows that production creates outside the model hooks are
reproduced by calling the same helper the write path calls:

* `IssueActivity` rows are written by calling `plane.bgtasks.issue_activity`
  (the Celery task) **synchronously** with `notification=False`. The task's own
  `bulk_create` is what inserts them, so the row shape is the task's, not a
  hand-written copy.
* Default project states are created with `State.objects.bulk_create` from
  `plane.db.models.state.DEFAULT_STATES`, exactly like
  `plane/app/views/project/base.py:282` does on project create (which is why
  they have an empty `slug`).
* `Profile`, `WorkspaceMember`, `ProjectMember` and friends are created
  explicitly because production creates them in the signup/workspace view,
  not in a hook.

Everything Celery would do over the broker is intercepted: `Task.delay` and
`Task.apply_async` are replaced for the whole build. `notifications` is a no-op
(the notification rows are written directly with the same shape the task
produces) and every other task is dropped, so the build never publishes to
RabbitMQ, never sends mail and never calls a webhook.

Determinism
-----------
The same `--base-time` must produce byte-identical tables. Every id is
`uuid5(NAMESPACE, "kind:name")`; every token, anchor, secret and page-log
transaction is derived from the same namespace; passwords are hashed with a
fixed salt (so `_auth_user_hash` in the sessions is stable); session payloads
are signed with `django.core.signing`'s clock pinned to `--base-time`; and
`_pin_timestamps` rewrites every `created_at`/`updated_at` (plus the per-model
`auto_now`/`default=now` columns) to values derived from `--base-time`.
`dev/bootstrap-fixture.sh` passes one `--base-time` per build; the determinism
check (`tools/fixtures/check_determinism.py`) passes the *same* value to two
builds and compares every table.

Cycle windows are relative to `--base-time` (`past`, `current`, `upcoming`,
`archived`); a scenario must not depend on the exact day a "current" cycle is
read, only on the ordering and the group membership the fixture guarantees.

Usage (normally via dev/bootstrap-fixture.sh):

    cd apps/api
    PLANE_DB=t_fix ... .venv/bin/python ../../apps/server/tools/fixtures/build.py \
        --database t_fix --base-time 2026-11-16T12:00:00Z \
        --manifest ../server/scenarios/fixtures/fix_tpl.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import uuid
from contextlib import contextmanager
from datetime import datetime, timedelta, timezone as dt_timezone
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
REPO_ROOT = SCRIPT.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"
DEFAULT_MANIFEST = SERVER_DIR / "scenarios" / "fixtures" / "fix_tpl.json"
TEMPLATE_DB = "plane_fix_tpl"

# A namespace of its own, so the read fixture's ids can never collide with the
# policy fixture's (6f1d3c2a-…) or with a uuid4 produced at runtime.
NAMESPACE = uuid.UUID("5b0a6a7e-0000-4000-8000-0000000f17a0")

# Every user gets this password. The fixture is a local dev template; the value
# is not a secret and appears in scenarios only as `password_vars` metadata.
PASSWORD = "Fix-Template-Passw0rd-2026!"
PASSWORD_SALT = "fixture-salt-0001"

# Tables whose rows come from the template clone untouched (migrations,
# instance registration, encrypted configuration). They are identical in every
# build and are not re-pinned.
TEMPLATE_TABLES = {
    "django_migrations",
    "django_content_type",
    "auth_permission",
    "auth_group",
    "auth_group_permissions",
    "instances",
    "instance_configurations",
    "django_celery_beat_clockedschedule",
    "django_celery_beat_crontabschedule",
    "django_celery_beat_intervalschedule",
    "django_celery_beat_periodictask",
    "django_celery_beat_periodictasks",
    "django_celery_beat_solarschedule",
}


def stable(name: str) -> uuid.UUID:
    """A deterministic id from a stable name.

    uuid5 from the fixed namespace gives the determinism; the value is then
    re-stamped as version 4 because Plane's `is_valid_uuid`
    (`plane/utils/uuid.py:8`) only accepts version-4 UUIDs. That helper guards
    the activity diffing (`issue_activity` returns early for a non-v4 project
    id) and every `track_*` parent/state/label id, so a plain uuid5 would make
    the real task write nothing. The bytes stay derived from `name`.
    """
    return uuid.UUID(uuid.uuid5(NAMESPACE, name).hex, version=4)


def stable_hex(name: str) -> str:
    return stable(name).hex


def iso(value: datetime) -> str:
    return value.astimezone(dt_timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def parse_base_time(value: str) -> datetime:
    text = value.strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    parsed = datetime.fromisoformat(text)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt_timezone.utc)
    return parsed.astimezone(dt_timezone.utc).replace(minute=0, second=0, microsecond=0)


# ---------------------------------------------------------------------------
# Environment guards
# ---------------------------------------------------------------------------


def database_name() -> str:
    import urllib.parse

    url = os.environ.get("DATABASE_URL")
    if not url:
        raise SystemExit(
            "DATABASE_URL is not set; run this through apps/server/dev/django-env.sh "
            "(dev/bootstrap-fixture.sh does)"
        )
    return urllib.parse.unquote(urllib.parse.urlparse(url).path.lstrip("/"))


def check_database(expected: str) -> None:
    name = database_name()
    if name != expected:
        raise SystemExit(f"DATABASE_URL points at {name!r}, not the expected {expected!r}")
    if name in {"plane", "plane_tpl", "postgres", "template0", "template1"}:
        raise SystemExit(f"refusing to build the fixture into protected database {name!r}")


def patch_celery() -> list[str]:
    """Replace every Celery task's dispatch with a local no-op.

    The build must not publish to RabbitMQ, send mail or call a webhook. The
    tasks whose rows we want are called directly (see the module docstring).
    """
    import celery

    patched: list[str] = []

    def noop(*_args, **_kwargs):
        return None

    for name, task in list(celery.current_app.tasks.items()):
        if not isinstance(task, celery.Task):
            continue
        try:
            task.delay = noop  # type: ignore[method-assign]
            task.apply_async = noop  # type: ignore[method-assign]
            patched.append(name)
        except Exception:
            continue
    return patched


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--database", required=True, help="database the build writes to (must match DATABASE_URL)")
    parser.add_argument(
        "--base-time",
        default=None,
        help="fixed instant every timestamp is derived from (default: now truncated to the hour)",
    )
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--no-manifest", action="store_true", help="do not write the manifest file")
    args = parser.parse_args()

    check_database(args.database)
    base = parse_base_time(args.base_time) if args.base_time else datetime.now(dt_timezone.utc).replace(
        minute=0, second=0, microsecond=0
    )

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    sys.path.insert(0, str(API_DIR))
    import django

    django.setup()

    patched = patch_celery()
    print(f"celery dispatch intercepted for {len(patched)} tasks", file=sys.stderr)

    from django.db import connection

    fixture = Fixture(base)
    with connection.cursor() as cursor:
        cursor.execute("SET statement_timeout = 0")
    fixture.build()
    fixture.pin_timestamps()
    fixture.create_sessions()
    fixture.count_rows()

    if not args.no_manifest:
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        args.manifest.write_text(
            json.dumps(fixture.manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        print(f"wrote {args.manifest}", file=sys.stderr)

    print(json.dumps({"base_time": fixture.manifest["base_time"], "vars": len(fixture.manifest["vars"])}))
    return 0


class Fixture:
    """Create the read fixture and record every id a scenario can reference."""

    def __init__(self, base: datetime) -> None:
        self.base = base
        self.manifest: dict = {
            "generated_by": "apps/server/tools/fixtures/build.py",
            "template_db": TEMPLATE_DB,
            "base_time": iso(base),
            "password": PASSWORD,
            "vars": {},
            "sessions": {},
            "counts": {},
            "notes": [],
        }
        self.v = self.manifest["vars"]
        self.objects: dict[str, dict] = {}
        self.tables: set[str] = set()
        self._passed_explicit_time: list[str] = []

    # -- helpers -----------------------------------------------------------

    def var(self, name: str, value) -> str:
        text = str(value)
        self.v[name] = text
        return text

    def remember(self, table: str) -> None:
        self.tables.add(table)

    def renumber(self, model, prefix: str, order_by: tuple[str, ...]) -> dict:
        """Replace auto-generated uuid4 primary keys with stable ones.

        A few rows are created by model hooks and Celery tasks that do not take
        an id (`Description` from `IssueComment.save`, `IssueSequence` from
        `Issue.save`, `IssueActivity` from the task, the notification
        preference from the `post_save` signal, `ProjectUserProperty` from
        `ProjectMember.save`). Their uuid4 ids would make two builds differ, so
        they are re-keyed in a deterministic order. `order_by` must be unique
        (or content-equivalent for identical rows) for the result to be stable.
        """
        rows = list(model.all_objects.order_by(*order_by).values_list("id", flat=True))
        mapping: dict = {}
        for index, old in enumerate(rows):
            new = stable(f"{prefix}:{index:05d}")
            mapping[old] = new
        for old, new in mapping.items():
            if old != new:
                model.all_objects.filter(pk=old).update(id=new)
        self.remember(model._meta.db_table)
        return mapping

    # -- build -------------------------------------------------------------

    def build(self) -> None:
        self.create_users()
        with self.acting_as("admin"):
            self.create_workspaces()
            self.create_projects()
            self.create_states()
            self.create_labels()
            self.create_estimates()
            self.create_issue_types()
            self.create_cycles()
            self.create_modules()
            self.create_issues()
            self.create_relations_links_attachments()
            self.create_comments()
            self.create_reactions_votes_subscribers()
            self.create_activities_versions()
            self.create_drafts()
            self.create_intake()
            self.create_pages()
            self.create_views()
            self.create_favorites()
            self.create_quick_links()
            self.create_stickies()
            self.create_recent_visits()
            self.create_user_properties()
            self.create_workspace_themes()
            self.create_accounts()
            self.create_webhooks()
            self.create_api_tokens()
            self.create_exporters()
            self.create_notifications()
            self.create_deploy_boards()
            self.create_instance_admins()

    @contextmanager
    def acting_as(self, name: str):
        """Run ORM writes with a current user, as a request would.

        `BaseModel.save` reads `crum.get_current_user()` (MODEL_HOOKS §1.6); with
        no current user it clears `created_by` and `updated_by`. Real rows are
        written inside requests or tasks, so the fixture installs the same
        thread-local override rather than passing `created_by_id` to every
        create (which Django's `objects.create()` would not forward to
        `save()` anyway).
        """
        import crum

        users = self.objects.get("users", {})
        user = users.get(name)
        previous = crum.get_current_user()
        crum.set_current_user(user)
        try:
            yield user
        finally:
            crum.set_current_user(previous)
    # -- users -------------------------------------------------------------

    def create_users(self) -> None:
        from django.contrib.auth.hashers import PBKDF2PasswordHasher

        from plane.db.models import Profile, User

        hasher = PBKDF2PasswordHasher()
        password_hash = hasher.encode(PASSWORD, PASSWORD_SALT)
        self.manifest["password_hash"] = password_hash

        specs = [
            # name, email, display name, timezone, language, flags
            ("admin", "fix.admin@example.com", "Ada Admin", "UTC", {}),
            ("member", "fix.member@example.com", "Mia Member", "Asia/Kolkata", {}),
            ("member2", "fix.member2@example.com", "Marco Member", "America/New_York", {}),
            ("guest", "fix.guest@example.com", "Gus Guest", "Europe/Berlin", {}),
            ("guest2", "fix.guest2@example.com", "Greta Guest", "Asia/Tokyo", {}),
            ("nonmember", "fix.nonmember@example.com", "Nina Nonmember", "UTC", {}),
            ("inactive", "fix.inactive@example.com", "Ivan Inactive", "UTC", {"is_active": False}),
            ("bot", "fix.bot@example.com", "Seed Bot", "UTC", {"is_bot": True, "bot_type": "WORKSPACE_SEED"}),
            ("deleted_member", "fix.deleted-member@example.com", "Delia Removed", "UTC", {}),
            ("invitee", "fix.invitee@example.com", "Ian Invitee", "Australia/Sydney", {}),
            ("tz_kolkata", "fix.kolkata@example.com", "Kavya Rao", "Asia/Kolkata", {}),
            ("tz_newyork", "fix.newyork@example.com", "Nora York", "America/New_York", {}),
            ("tz_berlin", "fix.berlin@example.com", "Bernd Berlin", "Europe/Berlin", {}),
            ("tz_tokyo", "fix.tokyo@example.com", "Tomo Tokyo", "Asia/Tokyo", {}),
            ("onboarded", "fix.onboarded@example.com", "Olive Onboarded", "UTC", {}),
            ("no_workspace", "fix.lonely@example.com", "Lonnie Lone", "Pacific/Auckland", {}),
            ("api_bot", "fix.api-bot@example.com", "Api Bot", "UTC", {"is_bot": True}),
        ]
        users: dict[str, User] = {}
        for index, (name, email, display, tz, flags) in enumerate(specs):
            user = User(
                id=stable(f"user:{name}"),
                username=stable_hex(f"user-username:{name}"),
                email=email,
                display_name=display,
                first_name=display.split(" ")[0],
                last_name=display.split(" ")[1],
                avatar="",
                last_location="",
                created_location="",
                token=stable_hex(f"user-token:{name}") + stable_hex(f"user-token2:{name}"),
                last_login_ip="203.0.113.10",
                last_logout_ip="203.0.113.11",
                last_login_uagent="Mozilla/5.0 (fix-fixture)",
                last_login_medium="email",
                user_timezone=tz,
                is_email_verified=True,
                is_email_valid=True,
                password=password_hash,
                **flags,
            )
            user.save()
            self.remember("users")
            users[name] = user
            self.var(f"user_{name}_id", user.id)
            self.var(f"user_{name}_email", email)
        self.objects["users"] = users

        # Profiles are created by the signup adapter, not by a signal
        # (plane/authentication/adapter/base.py). Set deterministic content.
        onboarding_steps = {
            "admin": {"profile_complete": True, "workspace_create": True, "workspace_invite": True, "workspace_join": True},
            "member": {"profile_complete": True, "workspace_create": True, "workspace_invite": True, "workspace_join": True},
            "member2": {"profile_complete": True, "workspace_create": False, "workspace_invite": False, "workspace_join": True},
            "guest": {"profile_complete": True, "workspace_create": True, "workspace_invite": False, "workspace_join": True},
            "onboarded": {"profile_complete": True, "workspace_create": True, "workspace_invite": True, "workspace_join": True},
        }
        for name, user in users.items():
            mobile = dict(
                profile_complete=name in onboarding_steps,
                workspace_create=name in onboarding_steps,
                workspace_join=name in onboarding_steps,
            )
            profile = Profile(
                id=stable(f"profile:{name}"),
                user=user,
                company_name="Plane Fixtures" if name != "no_workspace" else "",
                role="Engineering" if name in {"admin", "member"} else "",
                theme={"theme": "light", "colorSchema": "indigo", "primaryColor": "#6366F1"},
                background_color="#f4f5f7",
                goals={"goals": ["ship", "measure"]} if name == "admin" else {},
                onboarding_step=onboarding_steps.get(
                    name, {"profile_complete": False, "workspace_create": False, "workspace_invite": False, "workspace_join": False}
                ),
                mobile_onboarding_step=mobile,
                product_tour={
                    "work_items": name in onboarding_steps,
                    "cycles": name in onboarding_steps,
                    "modules": name in onboarding_steps,
                    "intake": name in onboarding_steps,
                    "pages": name in onboarding_steps,
                },
                is_tour_completed=name in onboarding_steps,
                language="en" if name not in {"tz_berlin"} else "de",
            )
            profile.save()
            self.remember("profiles")

    # -- workspaces --------------------------------------------------------

    def create_workspaces(self) -> None:
        from plane.db.models import Workspace, WorkspaceMember, WorkspaceMemberInvite

        users = self.objects["users"]
        specs = [
            ("ws", "fix-workspace", "Fixture Workspace", "admin", "Europe/Berlin", "51-100"),
            ("ws2", "fix-second", "Fixture Second", "nonmember", "America/New_York", "1-10"),
            ("ws_deleted", "fix-deleted", "Fixture Deleted", "admin", "UTC", "1-10"),
            # A workspace with `logo = NULL`. `Workspace.logo` is nullable and
            # `Workspace.logo_url` falls back to `self.logo`, which renders
            # `logo_url: null`; every other fixture workspace sets `""`, so this
            # is the only row that exercises the NULL branch of the nested
            # `WorkspaceLiteSerializer` (see `projects-extra.yaml`).
            ("ws_nologo", "fix-nologo", "Fixture No Logo", "admin", "UTC", "1-10"),
        ]
        workspaces: dict[str, Workspace] = {}
        for name, slug, title, owner, tz, size in specs:
            workspace = Workspace(
                id=stable(f"workspace:{name}"),
                name=title,
                slug=slug,
                owner=users[owner],
                timezone=tz,
                organization_size=size,
                background_color="#e6e7eb",
                logo=None if name == "ws_nologo" else "",
            )
            workspace.save()
            self.remember("workspaces")
            workspaces[name] = workspace
            self.var(name, slug)
            self.var(f"{name}_id", workspace.id)
            self.var(f"{name}_name", title)
        self.objects["workspaces"] = workspaces

        memberships = [
            # (workspace, user, role, is_active)
            ("ws", "admin", 20, True),
            ("ws", "member", 15, True),
            ("ws", "member2", 15, True),
            ("ws", "guest", 5, True),
            ("ws", "guest2", 5, True),
            ("ws", "inactive", 15, False),
            ("ws", "deleted_member", 15, True),
            ("ws", "tz_kolkata", 15, True),
            ("ws", "tz_newyork", 15, True),
            ("ws", "tz_berlin", 15, True),
            ("ws", "tz_tokyo", 5, True),
            ("ws", "onboarded", 15, True),
            ("ws2", "nonmember", 20, True),
            ("ws2", "member", 15, True),
            ("ws2", "tz_tokyo", 15, True),
            ("ws_deleted", "admin", 20, True),
            ("ws_deleted", "member", 15, True),
            ("ws_nologo", "admin", 20, True),
        ]
        for workspace_name, user_name, role, active in memberships:
            membership = WorkspaceMember(
                id=stable(f"workspace-member:{workspace_name}:{user_name}"),
                workspace=workspaces[workspace_name],
                member=users[user_name],
                role=role,
                company_role=f"{user_name} at fixtures",
                is_active=active,
                created_by_id=workspace_name == "ws" and users["admin"].id or users[user_name].id,
            )
            membership.save()
            self.remember("workspace_members")
            self.var(f"workspace_member_{workspace_name}_{user_name}", membership.id)

        # A soft-deleted membership: the bulk soft delete (one UPDATE) is what
        # `WorkspaceMember.objects.filter(...).delete()` does, so no hooks or
        # cascade task run.
        WorkspaceMember.objects.filter(
            workspace=workspaces["ws"], member=users["deleted_member"]
        ).update(deleted_at=self.base - timedelta(days=30))
        self.remember("workspace_members")

        # A soft-deleted workspace. `Workspace.delete()` would append
        # `__<epoch>` to the slug (workspace.py:156-176), which is a wall-clock
        # value; set `deleted_at` with a column update instead so the slug and
        # every row stay deterministic. The ledger records the deviation.
        Workspace.objects.filter(pk=workspaces["ws_deleted"].pk).update(
            deleted_at=self.base - timedelta(days=45)
        )
        self.remember("workspaces")
        self.manifest["notes"].extend(
            [
                "ws_deleted is soft-deleted with a column UPDATE instead of Workspace.delete(), "
                "because the override rewrites the slug with int(deleted_at.timestamp()) "
                "(plane/db/models/workspace.py:156-176), which is not reproducible.",
                "Ids are uuid5 from a fixed namespace, re-stamped as version 4: Plane's "
                "is_valid_uuid (plane/utils/uuid.py:8) only accepts v4 and issue_activity "
                "returns early for anything else.",
                "No session is created for the inactive user (Django refuses to authenticate an "
                "inactive user) or for the api_bot user (a bot has no interactive session).",
                "Invitation, API and exporter tokens are 64 lower-case hex characters so they "
                "match the `api_token` shape that scenarios/normalize.yaml applies to every JSON "
                "`token` key; the real invitation writer emits a JWT, which that shape rejects.",
            ]
        )

        invitations = [
            ("ws", "invitee", 15, False),
            ("ws", "new-hire@example.com", 5, False),
            ("ws2", "invitee", 15, False),
        ]
        for index, (workspace_name, email, role, accepted) in enumerate(invitations):
            value = (
                f"{workspace_name}:{email}"
                if email.startswith("fix.") or "@" in email
                else f"{workspace_name}:{email}"
            )
            invite = WorkspaceMemberInvite(
                id=stable(f"workspace-invite:{value}"),
                workspace=workspaces[workspace_name],
                email=email if "@" in email else users[email].email,
                role=role,
                accepted=accepted,
                token=stable_hex(f"workspace-invite-token:{index}") + stable_hex(f"workspace-invite-token2:{index}"),
                message="Join the fixture workspace",
                responded_at=None,
                created_by_id=users["admin"].id,
            )
            invite.save()
            self.remember("workspace_member_invites")
        self.var("invite_email", "fix.invitee@example.com")
        self.var(
            "workspace_invite_id",
            WorkspaceMemberInvite.objects.get(workspace=workspaces["ws"], email="fix.invitee@example.com").id,
        )

    # -- projects ----------------------------------------------------------

    def create_projects(self) -> None:
        from plane.db.models import Project, ProjectIdentifier, ProjectMember, ProjectMemberInvite

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        ws2 = self.objects["workspaces"]["ws2"]
        ws_nologo = self.objects["workspaces"]["ws_nologo"]
        specs = [
            # key, workspace, name, identifier, network, tz, archived, deleted
            ("alpha", ws, "Alpha Platform", "ALP", 2, "UTC", None, None),
            ("beta", ws, "Beta Mobile", "BET", 0, "Europe/Berlin", None, None),
            ("gamma", ws, "Gamma Archived", "GAM", 2, "UTC", 40, None),
            ("delta", ws, "Delta Deleted", "DEL", 2, "UTC", None, 20),
            ("epsilon", ws, "Epsilon Design", "EPS", 2, "Asia/Kolkata", None, None),
            ("zeta", ws, "Zeta Empty", "ZET", 2, "UTC", None, None),
            ("theta", ws2, "Theta Second", "THE", 2, "America/New_York", None, None),
            ("nologo", ws_nologo, "No Logo Project", "NLG", 2, "UTC", None, None),
        ]
        projects: dict[str, Project] = {}
        for key, workspace, name, identifier, network, tz, archived_days, deleted_days in specs:
            project = Project(
                id=stable(f"project:{key}"),
                workspace=workspace,
                name=name,
                identifier=identifier,
                description=f"{name} description",
                description_text={"type": "doc", "content": [{"type": "paragraph"}]},
                description_html={"type": "doc", "content": [{"type": "paragraph"}]},
                network=network,
                timezone=tz,
                default_assignee=users["member"] if key == "alpha" else None,
                project_lead=users["admin"] if key in {"alpha", "beta"} else None,
                emoji="🚀" if key == "alpha" else None,
                icon_prop={"name": "rocket", "color": "#F59E0B"} if key == "alpha" else None,
                logo_props={"in_use": "emoji", "emoji": {"value": "🚀"}} if key == "alpha" else {},
                cover_image="https://images.example.test/covers/alpha.png" if key == "alpha" else "",
                module_view=True,
                cycle_view=key in {"alpha", "beta"},
                issue_views_view=True,
                page_view=True,
                intake_view=key in {"alpha", "beta"},
                is_time_tracking_enabled=key == "alpha",
                is_issue_type_enabled=key == "alpha",
                guest_view_all_features=key == "epsilon",
                archive_in=0 if archived_days is None else 1,
                close_in=0,
                created_by_id=users["admin"].id,
                updated_by=users["admin"],
            )
            project.save()
            self.remember("projects")
            projects[key] = project
            self.var(f"project_{key}", project.id)
            self.var(f"project_{key}_id", project.id)
            self.var(f"project_{key}_identifier", identifier)
            self.var(f"project_{key}_name", name)

            identifier_row = ProjectIdentifier(
                project=project,
                name=identifier,
                workspace=workspace,
            )
            identifier_row.save()
            self.remember("project_identifiers")
        self.objects["projects"] = projects

        # Soft-deleted project: column update, same reason as the workspace.
        Project.objects.filter(pk=projects["delta"].pk).update(
            deleted_at=self.base - timedelta(days=20)
        )
        # Archived project: a real column the readers filter on.
        Project.objects.filter(pk=projects["gamma"].pk).update(
            archived_at=self.base - timedelta(days=40)
        )
        self.remember("projects")

        membership_specs = {
            "alpha": [
                ("admin", 20, True),
                ("member", 15, True),
                ("member2", 15, True),
                ("guest", 5, True),
                ("guest2", 5, True),
                ("tz_kolkata", 15, True),
                ("tz_newyork", 15, True),
                ("tz_berlin", 15, True),
                ("inactive", 15, False),
                ("deleted_member", 15, True),
            ],
            "beta": [
                ("admin", 20, True),
                ("member", 20, True),
                ("member2", 15, True),
                ("guest", 5, True),
                ("tz_tokyo", 15, True),
            ],
            "gamma": [("admin", 20, True), ("member", 15, True)],
            "delta": [("admin", 20, True), ("member", 15, True)],
            "epsilon": [("admin", 20, True), ("guest", 5, True), ("guest2", 5, True)],
            "zeta": [("admin", 20, True)],
            "theta": [("nonmember", 20, True), ("member", 15, True)],
            "nologo": [("admin", 20, True)],
        }
        for project_key, entries in membership_specs.items():
            for user_name, role, active in entries:
                membership = ProjectMember(
                    id=stable(f"project-member:{project_key}:{user_name}"),
                    project=projects[project_key],
                    workspace=projects[project_key].workspace,
                    member=users[user_name],
                    role=role,
                    is_active=active,
                    created_by_id=users["admin"].id,
                )
                membership.save()
                self.remember("project_members")
                self.var(f"project_member_{project_key}_{user_name}", membership.id)
        # Soft-deleted project membership.
        ProjectMember.objects.filter(
            project=projects["alpha"], member=users["deleted_member"]
        ).update(deleted_at=self.base - timedelta(days=15))

        invites = [
            ("alpha", "fix.invitee@example.com", 15),
            ("beta", "fix.invitee@example.com", 5),
        ]
        for index, (project_key, email, role) in enumerate(invites):
            invite = ProjectMemberInvite(
                id=stable(f"project-invite:{project_key}:{email}"),
                project=projects[project_key],
                workspace=projects[project_key].workspace,
                email=email,
                role=role,
                token=stable_hex(f"project-invite-token:{index}") + stable_hex(f"project-invite-token2:{index}"),
                message="Join the project",
                created_by_id=users["admin"].id,
            )
            invite.save()
            self.remember("project_member_invites")
        self.var(
            "project_invite_id",
            ProjectMemberInvite.objects.get(project=projects["alpha"], email="fix.invitee@example.com").id,
        )

        # `ProjectMember.save` creates one `ProjectUserProperty` per new
        # membership without an id (plane/db/models/project.py:276-292).
        # Re-key them deterministically; (project, user) is unique for a live
        # row. This must happen before `_pin_timestamps` ranks rows by id.
        from plane.db.models import ProjectUserProperty as ProjectUserPropertyModel

        self.renumber(ProjectUserPropertyModel, "project-user-property", ("project_id", "user_id"))

    # -- states ------------------------------------------------------------

    def create_states(self) -> None:
        from plane.db.models import State
        from plane.db.models.state import DEFAULT_STATES

        users = self.objects["users"]
        projects = self.objects["projects"]
        states: dict[tuple[str, str], State] = {}
        for project_key in ("alpha", "beta", "gamma", "delta", "epsilon", "theta"):
            project = projects[project_key]
            rows = []
            for template in DEFAULT_STATES:
                rows.append(
                    State(
                        id=stable(f"state:{project_key}:{template['name']}"),
                        project=project,
                        workspace=project.workspace,
                        name=template["name"],
                        description="",
                        color=template["color"],
                        sequence=template["sequence"],
                        group=template["group"],
                        default=template.get("default", False),
                        is_triage=template["group"] == "triage",
                        created_by_id=users["admin"].id,
                    )
                )
            # bulk_create bypasses State.save, so `slug` stays "" exactly as it
            # does on project create (plane/app/views/project/base.py:282).
            State.objects.bulk_create(rows)
            self.remember("states")
            for row in rows:
                states[(project_key, row.name)] = row
        # Extra states in alpha so every group has more than one member and the
        # non-default ones are not all created by the same manager query.
        extra = [
            ("alpha", "In Review", "started", 41000, "#8B5CF6"),
            ("alpha", "Blocked", "unstarted", 26000, "#EF4444"),
            ("alpha", "Won't Do", "cancelled", 56000, "#6B7280"),
            ("beta", "Ready for QA", "started", 46000, "#0EA5E9"),
        ]
        for project_key, name, group, sequence, colour in extra:
            project = projects[project_key]
            state = State.objects.create(
                id=stable(f"state:{project_key}:{name}"),
                project=project,
                workspace=project.workspace,
                name=name,
                description=f"{name} state",
                color=colour,
                sequence=sequence,
                group=group,
                created_by_id=users["member"].id,
            )
            states[(project_key, name)] = state
        self.remember("states")

        # A soft-deleted state.
        State.objects.filter(pk=states[("alpha", "Blocked")].pk).update(
            deleted_at=self.base - timedelta(days=10)
        )
        # The blocked state is referenced by no issue below, so a soft-deleted
        # state does not change other rows' interpretation.

        for (project_key, name), state in states.items():
            slug = name.lower().replace(" ", "-").replace("'", "")
            self.var(f"state_{project_key}_{slug.replace('-', '_')}", state.id)
        self.var("state_alpha_triage_id", states[("alpha", "Triage")].id)
        self.var("state_alpha_done_id", states[("alpha", "Done")].id)
        self.var("state_alpha_todo_id", states[("alpha", "Todo")].id)
        self.objects["states"] = states

        # Project.default_state (the model field; readers use it for
        # project serialisation and intake defaults).
        projects["alpha"].default_state = states[("alpha", "Backlog")]
        projects["alpha"].save(update_fields=["default_state"])
        projects["beta"].default_state = states[("beta", "Backlog")]
        projects["beta"].save(update_fields=["default_state"])

        # A deliberately un-slugged state is what bulk_create produces; record
        # one so a scenario can assert it.
        self.var("state_alpha_backlog_slug", states[("alpha", "Backlog")].slug)
    # -- labels ------------------------------------------------------------

    def create_labels(self) -> None:
        from plane.db.models import Label

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        projects = self.objects["projects"]
        labels: dict[str, Label] = {}

        workspace_labels = [
            ("cross_cutting", "Cross-cutting", "#0F172A", None),
            ("design_system", "Design System", "#DB2777", None),
        ]
        for key, name, colour, parent in workspace_labels:
            label = Label.objects.create(
                id=stable(f"label:ws:{key}"),
                workspace=ws,
                project=None,
                name=name,
                description=f"{name} workspace label",
                color=colour,
                created_by_id=users["admin"].id,
            )
            labels[key] = label
        self.remember("labels")

        project_labels = {
            "alpha": [
                ("bug", "Bug", "#EF4444", None),
                ("feature", "Feature", "#22C55E", None),
                ("regression", "Regression", "#F97316", "bug"),
                ("crash", "Crash", "#B91C1C", "bug"),
                ("ui", "UI", "#3B82F6", "feature"),
                ("api", "API", "#8B5CF6", "feature"),
                ("docs", "Docs", "#64748B", None),
            ],
            "beta": [
                ("mobile_bug", "Mobile Bug", "#EF4444", None),
                ("ios", "iOS", "#111827", "mobile_bug"),
                ("android", "Android", "#16A34A", "mobile_bug"),
            ],
            "gamma": [("legacy", "Legacy", "#9CA3AF", None)],
            "epsilon": [("brand", "Brand", "#EAB308", None)],
            "theta": [("second", "Second", "#0EA5E9", None)],
        }
        for project_key, entries in project_labels.items():
            project = projects[project_key]
            for key, name, colour, parent_key in entries:
                label = Label.objects.create(
                    id=stable(f"label:{project_key}:{key}"),
                    workspace=project.workspace,
                    project=project,
                    name=name,
                    description=f"{name} label",
                    color=colour,
                    parent=labels.get(parent_key) if parent_key else None,
                    created_by_id=users["admin"].id,
                )
                labels[f"{project_key}:{key}"] = label
        self.remember("labels")

        Label.objects.filter(pk=labels["alpha:docs"].pk).update(
            deleted_at=self.base - timedelta(days=5)
        )
        for key, label in labels.items():
            self.var(f"label_{key.replace(':', '_')}", label.id)
        self.objects["labels"] = labels

    # -- estimates ---------------------------------------------------------

    def create_estimates(self) -> None:
        from plane.db.models import Estimate, EstimatePoint

        users = self.objects["users"]
        projects = self.objects["projects"]
        estimates: dict[str, Estimate] = {}
        points: dict[str, EstimatePoint] = {}

        specs = [
            ("alpha:points", "alpha", "Story Points", "points", [("1", 1), ("2", 2), ("3", 3), ("5", 5), ("8", 8)]),
            ("alpha:categories", "alpha", "Complexity", "categories", [("Low", 1), ("Medium", 2), ("High", 3)]),
            ("beta:tshirt", "beta", "T-Shirt", "categories", [("XS", 1), ("S", 2), ("M", 3), ("L", 4), ("XL", 5)]),
        ]
        for key, project_key, name, estimate_type, values in specs:
            project = projects[project_key]
            estimate = Estimate.objects.create(
                id=stable(f"estimate:{key}"),
                project=project,
                workspace=project.workspace,
                name=name,
                description=f"{name} estimate",
                type=estimate_type,
                last_used=key == "alpha:points",
                created_by_id=users["admin"].id,
            )
            estimates[key] = estimate
            for index, (value, order) in enumerate(values):
                point = EstimatePoint.objects.create(
                    id=stable(f"estimate-point:{key}:{value}"),
                    project=project,
                    workspace=project.workspace,
                    estimate=estimate,
                    key=order,
                    description="",
                    value=value,
                    created_by_id=users["admin"].id,
                )
                points[f"{key}:{value}"] = point
        self.remember("estimates")
        self.remember("estimate_points")
        for key, estimate in estimates.items():
            self.var(f"estimate_{key.replace(':', '_')}", estimate.id)
        for key, point in points.items():
            self.var(f"estimate_point_{key.replace(':', '_').replace(' ', '_')}", point.id)
        self.objects["estimates"] = estimates
        self.objects["estimate_points"] = points

        projects["alpha"].estimate = estimates["alpha:points"]
        projects["alpha"].save(update_fields=["estimate"])
        projects["beta"].estimate = estimates["beta:tshirt"]
        projects["beta"].save(update_fields=["estimate"])

    # -- issue types -------------------------------------------------------

    def create_issue_types(self) -> None:
        from plane.db.models import IssueType
        from plane.db.models.issue_type import ProjectIssueType

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        projects = self.objects["projects"]
        types: dict[str, IssueType] = {}
        for key, name, is_epic, is_default, level in (
            ("task", "Task", False, True, 0),
            ("bug", "Bug", False, False, 0),
            ("epic", "Epic", True, False, 1),
        ):
            issue_type = IssueType.objects.create(
                id=stable(f"issue-type:{key}"),
                workspace=ws,
                name=name,
                description=f"{name} type",
                logo_props={"in_use": "icon", "icon": {"name": key}},
                is_epic=is_epic,
                is_default=is_default,
                level=level,
                created_by_id=users["admin"].id,
            )
            types[key] = issue_type
            self.var(f"issue_type_{key}", issue_type.id)
        self.remember("issue_types")
        for project_key in ("alpha", "beta"):
            for key, issue_type in types.items():
                row = ProjectIssueType.objects.create(
                    id=stable(f"project-issue-type:{project_key}:{key}"),
                    project=projects[project_key],
                    workspace=projects[project_key].workspace,
                    issue_type=issue_type,
                    level=int(issue_type.level),
                    is_default=key == "task",
                    created_by_id=users["admin"].id,
                )
                self.remember("project_issue_types")
                self.var(f"project_issue_type_{project_key}_{key}", row.id)
        self.objects["issue_types"] = types

    # -- cycles ------------------------------------------------------------

    def create_cycles(self) -> None:
        from plane.db.models import Cycle, CycleUserProperties

        users = self.objects["users"]
        projects = self.objects["projects"]
        cycles: dict[str, Cycle] = {}
        base = self.base
        specs = [
            # key, project, name, start offset (days), end offset, archived at
            ("alpha:past", "alpha", "Alpha Sprint 1", -120, -92, None),
            ("alpha:current", "alpha", "Alpha Sprint 2", -20, 20, None),
            ("alpha:upcoming", "alpha", "Alpha Sprint 3", 30, 58, None),
            ("alpha:archived", "alpha", "Alpha Sprint 0", -200, -170, -160),
            ("beta:past", "beta", "Beta Release 1", -80, -60, None),
            ("beta:current", "beta", "Beta Release 2", -10, 25, None),
            ("beta:upcoming", "beta", "Beta Release 3", 40, 70, None),
        ]
        for key, project_key, name, start, end, archived in specs:
            project = projects[project_key]
            cycle = Cycle.objects.create(
                id=stable(f"cycle:{key}"),
                project=project,
                workspace=project.workspace,
                name=name,
                description=f"{name} describes a {project.name} iteration",
                start_date=base + timedelta(days=start),
                end_date=base + timedelta(days=end),
                owned_by=users["admin"] if project_key == "alpha" else users["member"],
                timezone=project.timezone,
                view_props={"filters": {}, "display_filters": {"layout": "list"}},
                progress_snapshot={"completed": 3, "total": 12},
                logo_props={"in_use": "emoji", "emoji": {"value": "🔥"}},
                archived_at=(base + timedelta(days=archived)) if archived is not None else None,
                created_by_id=users["admin"].id,
            )
            cycles[key] = cycle
            self.var(f"cycle_{key.replace(':', '_')}", cycle.id)
            self.var(f"cycle_{key.replace(':', '_')}_name", name)
        self.remember("cycles")
        self.var("cycle_alpha_current_start", iso(base + timedelta(days=-20))[:10])
        self.var("cycle_alpha_current_end", iso(base + timedelta(days=20))[:10])
        self.objects["cycles"] = cycles

        for key, user_name in (
            ("alpha:current", "admin"),
            ("alpha:current", "member"),
            ("alpha:past", "admin"),
            ("beta:current", "member"),
        ):
            props = CycleUserProperties.objects.create(
                id=stable(f"cycle-user-properties:{key}:{user_name}"),
                project=cycles[key].project,
                workspace=cycles[key].workspace,
                cycle=cycles[key],
                user=users[user_name],
                filters={"priority": "high"},
                display_filters={"layout": "kanban", "group_by": "state"},
                display_properties={"assignee": False, "cycle": True, "estimate": True},
                rich_filters={"and": []},
            )
            self.remember("cycle_user_properties")
            self.var(f"cycle_user_properties_{key.replace(':', '_')}_{user_name}", props.id)

    # -- modules -----------------------------------------------------------

    def create_modules(self) -> None:
        from plane.db.models import Module, ModuleLink, ModuleMember, ModuleUserProperties

        users = self.objects["users"]
        projects = self.objects["projects"]
        modules: dict[str, Module] = {}
        base = self.base
        specs = [
            ("alpha:backlog", "alpha", "Alpha Backlog Module", "backlog"),
            ("alpha:planned", "alpha", "Alpha Planned Module", "planned"),
            ("alpha:in_progress", "alpha", "Alpha In Progress", "in-progress"),
            ("alpha:paused", "alpha", "Alpha Paused", "paused"),
            ("alpha:completed", "alpha", "Alpha Completed", "completed"),
            ("alpha:cancelled", "alpha", "Alpha Cancelled", "cancelled"),
            ("beta:planned", "beta", "Beta Planned Module", "planned"),
            ("beta:in_progress", "beta", "Beta In Progress", "in-progress"),
        ]
        for key, project_key, name, status in specs:
            project = projects[project_key]
            module = Module.objects.create(
                id=stable(f"module:{key}"),
                project=project,
                workspace=project.workspace,
                name=name,
                description=f"{name} description",
                description_text={"type": "doc", "content": []},
                description_html={"type": "doc", "content": []},
                start_date=(base + timedelta(days=-40)).date(),
                target_date=(base + timedelta(days=40)).date(),
                status=status,
                lead=users["admin"] if project_key == "alpha" else users["member"],
                view_props={"filters": {}, "display_filters": {"layout": "list"}},
                logo_props={"in_use": "emoji", "emoji": {"value": "📦"}},
                created_by_id=users["admin"].id,
            )
            modules[key] = module
            self.var(f"module_{key.replace(':', '_')}", module.id)
            self.var(f"module_{key.replace(':', '_')}_name", name)
            for user_name, role in (("admin", "lead"), ("member", "member"), ("guest", "viewer")):
                member = ModuleMember.objects.create(
                    id=stable(f"module-member:{key}:{user_name}"),
                    project=project,
                    workspace=project.workspace,
                    module=module,
                    member=users[user_name],
                    created_by_id=users["admin"].id,
                )
                self.remember("module_members")
                self.var(f"module_member_{key.replace(':', '_')}_{user_name}", member.id)
            props = ModuleUserProperties.objects.create(
                id=stable(f"module-user-properties:{key}:admin"),
                project=project,
                workspace=project.workspace,
                module=module,
                user=users["admin"],
                filters={"state_group": "started"},
                display_filters={"layout": "gantt"},
                display_properties={"assignee": True, "link": False, "state": True},
                rich_filters={"and": []},
            )
            self.remember("module_user_properties")
            self.var(f"module_user_properties_{key.replace(':', '_')}_admin", props.id)
        self.remember("modules")

        # An archived module.
        modules["alpha:archived"] = Module.objects.create(
            id=stable("module:alpha:archived"),
            project=projects["alpha"],
            workspace=projects["alpha"].workspace,
            name="Alpha Archived Module",
            description="Archived module",
            status="completed",
            archived_at=base - timedelta(days=25),
            created_by_id=users["admin"].id,
        )
        self.var("module_alpha_archived", modules["alpha:archived"].id)
        self.remember("modules")

        links = [
            ("alpha:in_progress", "Design doc", "https://docs.example.test/alpha/design"),
            ("alpha:in_progress", "Tracking sheet", "https://docs.example.test/alpha/tracking"),
            ("alpha:planned", "RFC", "https://docs.example.test/alpha/rfc"),
            ("beta:in_progress", "Release notes", "https://docs.example.test/beta/release"),
        ]
        for index, (module_key, title, url) in enumerate(links):
            link = ModuleLink.objects.create(
                id=stable(f"module-link:{index}:{module_key}"),
                project=modules[module_key].project,
                workspace=modules[module_key].workspace,
                module=modules[module_key],
                title=title,
                url=url,
                metadata={"title": title, "favicon": "https://docs.example.test/favicon.ico"},
                created_by_id=users["admin"].id,
            )
            self.remember("module_links")
            self.var(f"module_link_{index}", link.id)
        self.objects["modules"] = modules

    # -- issues ------------------------------------------------------------

    def issue_specs(self) -> list[dict]:
        """Deterministic issue content for ~400 rows.

        `state` names a key created by `create_states`; `project` a project
        key. The list is generated from fixed tables so every rebuild writes
        the same names, priorities and relations.
        """
        priorities = ["urgent", "high", "medium", "low", "none"]
        state_names = ["Backlog", "Todo", "In Progress", "Done", "Cancelled", "Triage"]
        titles = [
            "Add CSV export for filtered work items",
            "Cache the project activity feed",
            "Fix timezone drift in cycle boundaries",
            "Render mention chips in the comment editor",
            "Rate-limit the public intake endpoint",
            "Support nested labels in bulk edit",
            "Reduce N+1 queries on the issue list",
            "Show estimate rollups on module cards",
            "Handle duplicate webhook delivery",
            "Align filter chips with saved views",
            "Paginate workspace search results",
            "Add keyboard shortcut for new work item",
            "Surface overdue work items on the dashboard",
            "Persist board column order per user",
            "Validate S3 presign expiry",
            "Backfill issue sequence numbers",
            "Improve empty states for modules",
            "Migrate page locking to the new editor",
            "Expose cycle progress in the API",
            "Document the analytics query format",
            "Retry failed exporter jobs",
            "Group sub-issues under their parent",
            "Deduplicate notification rows",
            "Fix avatar uploads on Safari",
            "Support markdown in sticky notes",
            "Add workspace-level views",
            "Constrain guest access to assets",
            "Render the activity graph for a year",
            "Handle archived project reads",
            "Expose intake duplicate links",
        ]
        specs: list[dict] = []
        plan = [("alpha", 200), ("beta", 120), ("gamma", 30), ("delta", 20), ("epsilon", 30)]
        for project_key, count in plan:
            for index in range(count):
                title = titles[(index + len(project_key)) % len(titles)]
                specs.append(
                    {
                        "project": project_key,
                        "key": f"{project_key}:{index:03d}",
                        "name": f"{title} ({project_key.upper()}-{index:03d})",
                        "priority": priorities[index % len(priorities)],
                        "state_name": state_names[index % len(state_names)],
                        "suffix": index,
                    }
                )
        return specs

    def create_issues(self) -> None:
        from plane.db.models import (
            CycleIssue,
            Issue,
            IssueAssignee,
            IssueLabel,
            ModuleIssue,
            Project,
        )

        users = self.objects["users"]
        projects = self.objects["projects"]
        states: dict[tuple[str, str], object] = self.objects["states"]
        labels: dict[str, object] = self.objects["labels"]
        cycles: dict[str, object] = self.objects["cycles"]
        modules: dict[str, object] = self.objects["modules"]
        points: dict[str, object] = self.objects["estimate_points"]
        types: dict[str, object] = self.objects["issue_types"]
        base = self.base

        specs = self.issue_specs()
        issues: dict[str, Issue] = {}
        assignee_plan = {
            "alpha": ["admin", "member", "member2", "tz_kolkata", "tz_newyork", "tz_berlin"],
            "beta": ["member", "member2", "tz_tokyo"],
            "gamma": ["admin", "member"],
            "delta": ["admin", "member"],
            "epsilon": ["admin", "guest"],
        }
        label_plan = {
            "alpha": ["alpha:bug", "alpha:feature", "alpha:regression", "alpha:crash", "alpha:ui", "alpha:api", "cross_cutting", "design_system"],
            "beta": ["beta:mobile_bug", "beta:ios", "beta:android"],
            "gamma": ["gamma:legacy"],
            "delta": ["alpha:bug"],
            "epsilon": ["epsilon:brand"],
        }

        for spec in specs:
            project_key = spec["project"]
            project: Project = projects[project_key]
            suffix = spec["suffix"]
            state_name = spec["state_name"]
            if project_key == "alpha" and suffix % 17 == 0:
                state_name = "In Review"
            state = states[(project_key, state_name)]
            description_html = (
                "<p></p>"
                if suffix % 11 == 0
                else (
                    f"<p>{spec['name']}.</p>"
                    f"<p>Follow-up in <mention-component id=\"{users['member'].id}\"></mention-component>.</p>"
                    if suffix % 7 == 0
                    else f"<p>{spec['name']} needs work.</p>"
                )
            )
            issue = Issue(
                id=stable(f"issue:{spec['key']}"),
                project=project,
                workspace=project.workspace,
                name=spec["name"],
                state=state,
                priority=spec["priority"],
                description_html=description_html,
                description_json={"type": "doc", "content": []} if description_html == "<p></p>" else {"type": "doc", "content": [{"type": "paragraph"}]},
                sequence_id=1,  # overwritten by Issue.save under the advisory lock
                start_date=(base + timedelta(days=-(suffix % 90))).date() if suffix % 2 == 0 else None,
                target_date=(base + timedelta(days=(suffix % 60) - 30)).date() if suffix % 3 else None,
                estimate_point=points["alpha:points:3"] if project_key == "alpha" and suffix % 4 == 0 else None,
                type=types["bug"] if project_key == "alpha" and suffix % 5 == 0 else None,
                external_source="fixture" if suffix % 13 == 0 else None,
                external_id=f"ext-{spec['key']}" if suffix % 13 == 0 else None,
                created_by_id=users["admin" if suffix % 3 else "member"].id,
            )
            issue.save()
            issues[spec["key"]] = issue
            self.remember("issues")

            if suffix % 4 != 1:
                assignees = assignee_plan[project_key]
                picked = [assignees[suffix % len(assignees)], assignees[(suffix + 1) % len(assignees)]]
                for user_name in dict.fromkeys(picked):
                    row = IssueAssignee.objects.create(
                        id=stable(f"issue-assignee:{spec['key']}:{user_name}"),
                        project=project,
                        workspace=project.workspace,
                        issue=issue,
                        assignee=users[user_name],
                        created_by_id=users["admin"].id,
                    )
                    self.remember("issue_assignees")
            if suffix % 3 != 2:
                label_keys = label_plan[project_key]
                for label_key in [label_keys[suffix % len(label_keys)]]:
                    label = labels[label_key]
                    row = IssueLabel.objects.create(
                        id=stable(f"issue-label:{spec['key']}:{label_key}"),
                        project=project,
                        workspace=project.workspace,
                        issue=issue,
                        label=label,
                        created_by_id=users["admin"].id,
                    )
                    self.remember("issue_labels")

        self.objects["issues"] = issues

        # Parent/sub-issue trees: 12 parents with 2-3 children in alpha, and one
        # in beta. The child rows were already created above; wire `parent`.
        parent_keys = [f"alpha:{index:03d}" for index in range(0, 120, 10)]
        for parent_index, parent_key in enumerate(parent_keys):
            for offset in range(2 + (parent_index % 2)):
                child_key = f"alpha:{parent_index * 10 + 1 + offset:03d}"
                if child_key in issues:
                    child = issues[child_key]
                    child.parent = issues[parent_key]
                    child.save(update_fields=["parent"])
        beta_parent = issues["beta:000"]
        for child_key in ("beta:001", "beta:002"):
            child = issues[child_key]
            child.parent = beta_parent
            child.save(update_fields=["parent"])

        # Cycle issues: every third issue joins a cycle, the rest stay out.
        cycle_plan = {
            "alpha": ["alpha:past", "alpha:current", "alpha:upcoming", "alpha:archived"],
            "beta": ["beta:past", "beta:current", "beta:upcoming"],
        }
        for project_key, cycle_keys in cycle_plan.items():
            keys = [key for key in issues if key.startswith(project_key + ":")]
            for index, issue_key in enumerate(keys):
                if index % 3 != 0:
                    continue
                cycle_key = cycle_keys[(index // 3) % len(cycle_keys)]
                row = CycleIssue.objects.create(
                    id=stable(f"cycle-issue:{issue_key}:{cycle_key}"),
                    project=issues[issue_key].project,
                    workspace=issues[issue_key].workspace,
                    issue=issues[issue_key],
                    cycle=cycles[cycle_key],
                    created_by_id=users["admin"].id,
                )
                self.remember("cycle_issues")
        # A soft-deleted cycle-issue link, so readers that filter on
        # `deleted_at__isnull` are exercised.
        CycleIssue.objects.filter(
            id=stable("cycle-issue:alpha:003:alpha:past")
        ).update(deleted_at=self.base - timedelta(days=4))

        # Module issues: every fifth issue of alpha belongs to a module.
        module_plan = [
            ("alpha:backlog", [f"alpha:{i:03d}" for i in range(0, 200, 25)]),
            ("alpha:in_progress", [f"alpha:{i:03d}" for i in range(5, 200, 25)]),
            ("alpha:planned", [f"alpha:{i:03d}" for i in range(10, 200, 25)]),
            ("alpha:completed", [f"alpha:{i:03d}" for i in range(15, 200, 25)]),
            ("alpha:cancelled", [f"alpha:{i:03d}" for i in range(20, 200, 25)]),
            ("alpha:paused", [f"alpha:{i:03d}" for i in range(1, 200, 50)]),
            ("beta:planned", [f"beta:{i:03d}" for i in range(0, 120, 20)]),
            ("beta:in_progress", [f"beta:{i:03d}" for i in range(7, 120, 20)]),
        ]
        for module_key, issue_keys in module_plan:
            for issue_key in issue_keys:
                issue = issues[issue_key]
                row = ModuleIssue.objects.create(
                    id=stable(f"module-issue:{module_key}:{issue_key}"),
                    project=issue.project,
                    workspace=issue.workspace,
                    module=modules[module_key],
                    issue=issue,
                    created_by_id=users["admin"].id,
                )
                self.remember("module_issues")

        # Flagship issues referenced by the scenarios.
        flagship = {
            "issue_alpha": "alpha:000",
            "issue_alpha_completed": "alpha:003",
            "issue_alpha_overdue": "alpha:005",
            "issue_alpha_child": "alpha:001",
            "issue_alpha_parent": "alpha:000",
            "issue_beta": "beta:000",
            "issue_gamma": "gamma:000",
            "issue_delta": "delta:000",
            "issue_epsilon": "epsilon:000",
        }
        for name, key in flagship.items():
            issue = issues[key]
            self.var(name, issue.id)
            self.var(f"{name}_name", issue.name)
            self.var(f"{name}_sequence", issue.sequence_id)
            self.var(f"{name}_identifier", f"{issue.project.identifier}-{issue.sequence_id}")

        # Archived issues: column update is what the archive view's bulk_update
        # does (it bypasses Issue.save, MODLE_HOOKS §4.1).
        for key in ("alpha:004", "alpha:104", "beta:004"):
            Issue.objects.filter(pk=issues[key].pk).update(archived_at=(base - timedelta(days=12)).date())
        self.remember("issues")
        self.var("issue_alpha_archived", issues["alpha:004"].id)
        self.var("issue_beta_archived", issues["beta:004"].id)

        # Soft-deleted issues: one via the bulk soft delete (queryset), which
        # matches `Issue.objects.filter(...).delete()`.
        Issue.objects.filter(pk__in=[issues["alpha:105"].pk, issues["beta:005"].pk]).update(
            deleted_at=base - timedelta(days=8)
        )
        self.var("issue_alpha_deleted", issues["alpha:105"].id)
        self.var("issue_beta_deleted", issues["beta:005"].id)

        # A "done" issue in a completed state has completed_at set by
        # Issue._sync_completed_at; pinning below makes it deterministic.
        self.var("issue_alpha_done", issues["alpha:003"].id)

        # Draft issues (is_draft): the readers exclude them from issue lists
        # but the issue detail and draft endpoints return them.
        draft = Issue.objects.create(
            id=stable("issue:alpha:draft"),
            project=projects["alpha"],
            workspace=projects["alpha"].workspace,
            name="Draft: consider offline mode",
            state=states[("alpha", "Backlog")],
            priority="low",
            description_html="<p>Draft description</p>",
            is_draft=True,
            created_by_id=users["member"].id,
        )
        self.remember("issues")
        self.var("issue_alpha_draft", draft.id)

        # Soft-deleted cycle links and module links are queried by the
        # respective `deleted_at__isnull` filters; the rows above are enough.

        # Issue identifiers used by the identifier routes.
        # The issue that carries the fixture's comments, links, attachments,
        # reactions, votes and activity history (alpha:010).
        self.var("issue_alpha_commented", issues["alpha:010"].id)
        # Every third issue joins a cycle, so alpha:003 is in the "current" one.
        self.var("issue_in_current_cycle", issues["alpha:003"].id)
        self.var("issue_alpha_identifier_path", f"{projects['alpha'].identifier}-{issues['alpha:001'].sequence_id}")
        self.var("issue_beta_identifier_path", f"{projects['beta'].identifier}-{issues['beta:001'].sequence_id}")
        self.var("work_item_alpha_path", f"{projects['alpha'].identifier}-{issues['alpha:002'].sequence_id}")

        # `Issue.save` writes one `IssueSequence` per new issue with no id;
        # re-key them deterministically. (project, sequence) is unique because
        # the sequence is monotonic per project.
        from plane.db.models import IssueSequence

        self.renumber(IssueSequence, "issue-sequence", ("project_id", "sequence"))

    # -- relations, links, attachments -------------------------------------

    def create_relations_links_attachments(self) -> None:
        from plane.db.models import FileAsset, Issue, IssueLink, IssueRelation

        users = self.objects["users"]
        issues: dict[str, Issue] = self.objects["issues"]
        projects = self.objects["projects"]
        base = self.base

        relation_types = [
            "blocked_by",
            "blocking",
            "relates_to",
            "duplicate",
            "start_before",
            "start_after",
            "finish_before",
            "finish_after",
            "implemented_by",
            "implements",
        ]
        pairs = [
            ("alpha:010", "alpha:011"),
            ("alpha:012", "alpha:013"),
            ("alpha:014", "alpha:015"),
            ("alpha:016", "alpha:017"),
            ("alpha:018", "alpha:019"),
            ("alpha:020", "alpha:021"),
            ("alpha:022", "alpha:023"),
            ("alpha:024", "alpha:025"),
            ("alpha:026", "alpha:027"),
            ("alpha:028", "alpha:029"),
            ("beta:010", "beta:011"),
            ("beta:012", "beta:013"),
        ]
        relations = {}
        for index, (left, right) in enumerate(pairs):
            relation_type = relation_types[index % len(relation_types)]
            issue = issues[left]
            relation = IssueRelation.objects.create(
                id=stable(f"issue-relation:{left}:{right}"),
                project=issue.project,
                workspace=issue.workspace,
                issue=issue,
                related_issue=issues[right],
                relation_type=relation_type,
                created_by_id=users["admin"].id,
            )
            relations[index] = relation
            self.remember("issue_relations")
        self.var("issue_relation_blocked_by", relations[0].id)
        self.var("issue_relation_blocking", relations[1].id)
        self.var("issue_relation_duplicate", relations[3].id)
        self.var("issue_relation_relates_to", relations[2].id)

        IssueRelation.objects.filter(pk=relations[4].pk).update(deleted_at=base - timedelta(days=2))

        link_targets = [
            ("alpha:010", "Design spec", "https://docs.example.test/specs/design"),
            ("alpha:011", "Bug report", "https://github.com/example/repo/issues/42"),
            ("alpha:012", "Figma", "https://figma.example.test/file/abc"),
            ("beta:010", "Release notes", "https://docs.example.test/beta/notes"),
        ]
        links = {}
        for index, (issue_key, title, url) in enumerate(link_targets):
            issue = issues[issue_key]
            link = IssueLink.objects.create(
                id=stable(f"issue-link:{index}"),
                project=issue.project,
                workspace=issue.workspace,
                issue=issue,
                title=title,
                url=url,
                metadata={"title": title, "favicon": "https://docs.example.test/favicon.ico"},
                created_by_id=users["admin"].id,
            )
            links[index] = link
            self.remember("issue_links")
        self.var("issue_link_design", links[0].id)
        self.var("issue_link_github", links[1].id)

        # FileAsset rows for attachments. The S3 object does not exist; asset
        # views presign or redirect and the harness never fetches the bytes.
        attachment_specs = [
            ("alpha:010", "spec.pdf", 10240, "application/pdf"),
            ("alpha:010", "mockup.png", 20480, "image/png"),
            ("alpha:011", "trace.log", 5120, "text/plain"),
            ("beta:010", "crash.txt", 2048, "text/plain"),
        ]
        asset_ids = []
        for index, (issue_key, filename, size, content_type) in enumerate(attachment_specs):
            issue = issues[issue_key]
            key = f"{stable_hex(f'asset-path:{index}')}-{filename}"
            asset = FileAsset.objects.create(
                id=stable(f"file-asset:attachment:{index}"),
                workspace=issue.workspace,
                project=issue.project,
                issue=issue,
                entity_type="ISSUE_ATTACHMENT",
                entity_identifier=str(issue.id),
                asset=f"{issue.workspace.id}/{key}",
                size=size,
                attributes={"name": filename, "type": content_type, "size": size},
                is_uploaded=True,
                created_by_id=users["admin"].id,
            )
            asset_ids.append(asset.id)
            self.remember("file_assets")
            self.var(f"attachment_asset_key_{index}", key)
        self.var("attachment_asset_key", f"{stable_hex('asset-path:0')}-spec.pdf")
        self.var("attachment_id", asset_ids[0])
        self.var("attachment_image_id", asset_ids[1])
        self.var("attachment_other_id", asset_ids[2])

        description_assets = [
            ("alpha:010", "ISSUE_DESCRIPTION", "diagram.png", 4096, "image/png"),
            ("alpha:011", "COMMENT_DESCRIPTION", "note.txt", 1024, "text/plain"),
        ]
        for index, (issue_key, entity_type, filename, size, content_type) in enumerate(description_assets):
            issue = issues[issue_key]
            asset = FileAsset.objects.create(
                id=stable(f"file-asset:description:{index}"),
                workspace=issue.workspace,
                project=issue.project,
                issue=issue,
                entity_type=entity_type,
                entity_identifier=str(issue.id),
                asset=f"{issue.workspace.id}/{stable_hex(f'asset-desc-path:{index}')}-{filename}",
                size=size,
                attributes={"name": filename, "type": content_type, "size": size},
                is_uploaded=True,
                created_by_id=users["admin"].id,
            )
            self.remember("file_assets")
            self.var(f"description_asset_{index}", asset.id)

        # A workspace logo asset and two user avatars, so the static asset
        # endpoints have something to serve (redirects only).
        from plane.db.models import Workspace

        ws = self.objects["workspaces"]["ws"]
        logo = FileAsset.objects.create(
            id=stable("file-asset:workspace-logo"),
            workspace=ws,
            entity_type="WORKSPACE_LOGO",
            entity_identifier=str(ws.id),
            asset=f"{ws.id}/{stable_hex('asset-ws-logo')}-logo.png",
            size=3072,
            attributes={"name": "logo.png", "type": "image/png", "size": 3072},
            is_uploaded=True,
            created_by_id=users["admin"].id,
        )
        self.remember("file_assets")
        self.var("workspace_logo_asset", logo.id)
        Workspace.objects.filter(pk=ws.pk).update(logo_asset=logo)

        for user_name in ("admin", "member"):
            user = users[user_name]
            asset = FileAsset.objects.create(
                id=stable(f"file-asset:avatar:{user_name}"),
                user=user,
                entity_type="USER_AVATAR",
                entity_identifier=str(user.id),
                asset=f"user-{stable_hex(f'asset-avatar:{user_name}')}-avatar.png",
                size=2048,
                attributes={"name": "avatar.png", "type": "image/png", "size": 2048},
                is_uploaded=True,
                created_by_id=user.id,
            )
            self.remember("file_assets")
            self.var(f"avatar_asset_{user_name}", asset.id)
            self.var(f"avatar_asset_key_{user_name}", f"user-{stable_hex(f'asset-avatar:{user_name}')}-avatar.png")
            type(user).objects.filter(pk=user.pk).update(avatar_asset=asset)

        cover = FileAsset.objects.create(
            id=stable("file-asset:project-cover"),
            workspace=ws,
            project=projects["alpha"],
            entity_type="PROJECT_COVER",
            entity_identifier=str(projects["alpha"].id),
            asset=f"{ws.id}/{stable_hex('asset-project-cover')}-cover.png",
            size=8192,
            attributes={"name": "cover.png", "type": "image/png", "size": 8192},
            is_uploaded=True,
            created_by_id=users["admin"].id,
        )
        self.remember("file_assets")
        self.var("project_cover_asset", cover.id)
        projects["alpha"].cover_image_asset = cover
        projects["alpha"].save(update_fields=["cover_image_asset"])

    # -- comments ----------------------------------------------------------

    def create_comments(self) -> None:
        from plane.db.models import DescriptionVersion, Issue, IssueComment

        users = self.objects["users"]
        issues: dict[str, Issue] = self.objects["issues"]
        base = self.base

        comment_targets = [
            "alpha:010",
            "alpha:010",
            "alpha:011",
            "alpha:012",
            "alpha:013",
            "beta:010",
            "gamma:000",
        ]
        comments = {}
        for index, issue_key in enumerate(comment_targets):
            issue = issues[issue_key]
            actor = users[["member", "admin", "guest", "member2"][index % 4]]
            comment = IssueComment.objects.create(
                id=stable(f"issue-comment:{index}"),
                project=issue.project,
                workspace=issue.workspace,
                issue=issue,
                actor=actor,
                comment_html=(
                    "<p>Looks good to me.</p>"
                    if index % 3
                    else f"<p>Ping <mention-component id=\"{users['admin'].id}\"></mention-component> about this.</p>"
                ),
                comment_json={"type": "doc", "content": [{"type": "paragraph"}]},
                # The space (public board) API only returns comments with
                # access="EXTERNAL" (plane/space/views/issue.py:96,271).
                access="EXTERNAL" if index in (0, 4) else "INTERNAL",
                created_by_id=actor.id,
            )
            comments[index] = comment
            self.remember("issue_comments")
        self.var("comment_alpha", comments[0].id)
        self.var("comment_alpha_second", comments[1].id)
        self.var("comment_beta", comments[5].id)
        self.var("comment_public", comments[4].id)

        # A reply, so the comment tree has a parent.
        reply = IssueComment.objects.create(
            id=stable("issue-comment:reply"),
            project=issues["alpha:010"].project,
            workspace=issues["alpha:010"].workspace,
            issue=issues["alpha:010"],
            actor=users["admin"],
            parent=comments[0],
            comment_html="<p>Agreed, I picked it up.</p>",
            access="INTERNAL",
            created_by_id=users["admin"].id,
        )
        self.remember("issue_comments")
        self.var("comment_reply", reply.id)

        # A second external comment on the published issue, so the public
        # comment list and its reactions have more than one row.
        external = IssueComment.objects.create(
            id=stable("issue-comment:external"),
            project=issues["alpha:010"].project,
            workspace=issues["alpha:010"].workspace,
            issue=issues["alpha:010"],
            actor=users["guest"],
            comment_html="<p>Public board comment from a guest.</p>",
            access="EXTERNAL",
            created_by_id=users["guest"].id,
        )
        self.remember("issue_comments")
        self.var("comment_external", external.id)

        # `IssueComment.save` creates its `Description` with an auto uuid4 and
        # writes the id back with a second save. Re-key those descriptions
        # deterministically and point the comments at them (nothing else
        # references the descriptions yet).
        from plane.db.models import Description
        from django.db import transaction

        descriptions_by_comment = {}
        with transaction.atomic():
            for index, comment in enumerate(list(comments.values()) + [reply, external]):
                old_id = IssueComment.objects.filter(pk=comment.pk).values_list("description_id", flat=True).first()
                if old_id is None:
                    continue
                new_id = stable(f"comment-description:{index:02d}")
                Description.objects.filter(pk=old_id).update(id=new_id)
                IssueComment.objects.filter(pk=comment.pk).update(description_id=new_id)
                descriptions_by_comment[comment.pk] = new_id
                self.var(f"comment_description_{index}", new_id)
        self.remember("descriptions")
        self.remember("issue_comments")

        # Description versions: what `DescriptionVersion` rows look like after
        # a description edit. Written directly because the task that writes
        # them in production (`issue_description_version_sync`) is a Celery
        # task with the same row shape.
        version_index = 0
        for comment_id, description_id in descriptions_by_comment.items():
            for revision in range(2):
                version = DescriptionVersion.objects.create(
                    id=stable(f"description-version:{version_index}:{revision}"),
                    workspace=issues["alpha:010"].workspace,
                    description_id=description_id,
                    description_html=f"<p>Revision {revision} of the comment.</p>",
                    description_json={"type": "doc", "content": [{"type": "paragraph"}]},
                    created_by_id=users["admin"].id,
                )
                self.remember("description_versions")
                self.var(f"description_version_{version_index}_{revision}", version.id)
            version_index += 1

    # -- reactions, votes, subscribers -------------------------------------

    def create_reactions_votes_subscribers(self) -> None:
        from plane.db.models import (
            CommentReaction,
            Issue,
            IssueMention,
            IssueReaction,
            IssueSubscriber,
            IssueVote,
        )

        users = self.objects["users"]
        issues: dict[str, Issue] = self.objects["issues"]
        comments = {
            "alpha": self.v["comment_alpha"],
            "alpha_second": self.v["comment_alpha_second"],
            "beta": self.v["comment_beta"],
            "public": self.v["comment_public"],
        }
        reactions = ["thumbs_up", "thumbs_down", "smile", "heart", "eyes"]
        index = 0
        for issue_key in ("alpha:010", "alpha:011", "alpha:012", "beta:010", "epsilon:000"):
            issue = issues[issue_key]
            for user_name in ("admin", "member", "guest"):
                if (index + len(user_name)) % 3 == 0:
                    continue
                reaction = IssueReaction.objects.create(
                    id=stable(f"issue-reaction:{issue_key}:{user_name}"),
                    project=issue.project,
                    workspace=issue.workspace,
                    issue=issue,
                    actor=users[user_name],
                    reaction=reactions[index % len(reactions)],
                    created_by_id=users[user_name].id,
                )
                self.remember("issue_reactions")
                index += 1
            for user_name in ("admin", "member"):
                vote = IssueVote.objects.create(
                    id=stable(f"issue-vote:{issue_key}:{user_name}"),
                    project=issue.project,
                    workspace=issue.workspace,
                    issue=issue,
                    actor=users[user_name],
                    vote=1,
                    created_by_id=users[user_name].id,
                )
                self.remember("issue_votes")
            for user_name in ("admin", "member", "member2"):
                subscriber = IssueSubscriber.objects.create(
                    id=stable(f"issue-subscriber:{issue_key}:{user_name}"),
                    project=issue.project,
                    workspace=issue.workspace,
                    issue=issue,
                    subscriber=users[user_name],
                    created_by_id=users[user_name].id,
                )
                self.remember("issue_subscribers")
            for user_name in ("admin", "guest"):
                mention = IssueMention.objects.create(
                    id=stable(f"issue-mention:{issue_key}:{user_name}"),
                    project=issue.project,
                    workspace=issue.workspace,
                    issue=issue,
                    mention=users[user_name],
                    created_by_id=users["member"].id,
                )
                self.remember("issue_mentions")
        self.var("issue_reaction_thumbs_up", IssueReaction.objects.filter(reaction="thumbs_up").first().id)
        self.var("issue_vote_alpha", IssueVote.objects.filter(issue=issues["alpha:010"]).first().id)
        self.var("issue_subscriber_alpha", IssueSubscriber.objects.filter(issue=issues["alpha:010"]).first().id)
        self.var("issue_mention_alpha", IssueMention.objects.filter(issue=issues["alpha:010"]).first().id)

        comment_rows = dict(comments)
        comment_rows["external"] = self.v["comment_external"]
        from plane.db.models import IssueComment

        for comment_key, comment_id in comment_rows.items():
            comment = IssueComment.objects.get(pk=comment_id)
            for user_name in ("admin", "member"):
                reaction = CommentReaction.objects.create(
                    id=stable(f"comment-reaction:{comment_key}:{user_name}"),
                    project=comment.project,
                    workspace=comment.workspace,
                    comment_id=comment_id,
                    actor=users[user_name],
                    reaction="thumbs_up",
                    created_by_id=users[user_name].id,
                )
                self.remember("comment_reactions")
                self.var(f"comment_reaction_{comment_key}_{user_name}", reaction.id)

    # -- activities and versions -------------------------------------------

    def create_activities_versions(self) -> None:
        from plane.bgtasks.issue_activities_task import issue_activity
        from plane.db.models import Issue, IssueActivity, IssueDescriptionVersion, IssueVersion

        users = self.objects["users"]
        issues: dict[str, Issue] = self.objects["issues"]
        states = self.objects["states"]
        epoch = int(self.base.timestamp())

        # `issue_activity` is the Celery task; called synchronously it writes
        # the same IssueActivity rows the worker would (notification=False, so
        # it does not fan out into the notification task).
        def activity(kind: str, issue_key: str, requested: dict, current: dict | None, actor: str, **extra) -> None:
            issue = issues[issue_key]
            issue_activity(
                type=kind,
                requested_data=json.dumps(requested),
                current_instance=json.dumps(current) if current is not None else None,
                issue_id=str(issue.id),
                actor_id=str(users[actor].id),
                project_id=str(issue.project_id),
                epoch=epoch,
                notification=False,
                origin=None,
                **extra,
            )
            self.remember("issue_activities")

        created_keys = [
            "alpha:000",
            "alpha:010",
            "alpha:011",
            "alpha:012",
            "alpha:020",
            "beta:000",
            "beta:010",
            "epsilon:000",
        ]
        for issue_key in created_keys:
            activity(
                "issue.activity.created",
                issue_key,
                {"assignee_ids": [], "issue_id": str(issues[issue_key].id)},
                None,
                "admin",
            )
            self.remember("issue_activities")

        updated = [
            ("alpha:010", "state_id", {"state_id": str(states[("alpha", "In Progress")].id)}, {"state_id": str(states[("alpha", "Todo")].id)}),
            ("alpha:011", "priority", {"priority": "urgent"}, {"priority": "low"}),
            ("alpha:012", "name", {"name": "Renamed issue"}, {"name": "Original name"}),
            ("alpha:013", "target_date", {"target_date": "2027-01-15"}, {"target_date": None}),
            ("alpha:014", "labels", {"label_ids": []}, {"label_ids": []}),
            ("beta:010", "state_id", {"state_id": str(states[("beta", "Done")].id)}, {"state_id": str(states[("beta", "Todo")].id)}),
        ]
        for issue_key, _field, requested, current in updated:
            activity("issue.activity.updated", issue_key, requested, current, "member")
        for issue_key in ("alpha:010", "alpha:011", "beta:010"):
            activity("issue.activity.updated", issue_key, {"description_html": "<p>Edited</p>"}, {"description_html": "<p>Original</p>"}, "member")

        # Comment activity for the first comment of each thread.
        from plane.db.models import IssueComment

        for issue_key in ("alpha:010", "beta:010"):
            comment = IssueComment.objects.filter(issue=issues[issue_key]).first()
            activity(
                "comment.activity.created",
                issue_key,
                {"id": str(comment.id), "comment_html": comment.comment_html},
                None,
                "member",
            )
        activity(
            "issue_activity.created",
            "alpha:010",
            {"cycle_id": str(self.v["cycle_alpha_current"]), "cycle_name": "Alpha Sprint 2"},
            {"cycle_id": None},
            "admin",
        )

        # `IssueActivity` rows are created by the task without an id (one row
        # per "created" event through `.objects.create`, the rest through
        # `bulk_create`). Re-key them before the version snapshots read the
        # latest activity id. The order is content-based; two rows that are
        # identical except for the id would swap ids and still compare equal.
        self.renumber(
            IssueActivity,
            "issue-activity",
            ("issue_id", "epoch", "verb", "field", "comment", "old_value", "new_value", "issue_comment_id"),
        )

        # IssueVersion snapshots: production writes them with the
        # `sync_issue_version` task, which builds rows with
        # `create_issue_version()` and `bulk_create`. (The model classmethod
        # `IssueVersion.log_issue_version` is broken upstream: it calls
        # `apps.get_model` on the `django.apps` module instead of the registry,
        # plane/db/models/issue.py:743, so it logs and writes nothing.)
        from plane.bgtasks.issue_version_sync import create_issue_version, get_related_data

        version_targets = [issues[key] for key in ("alpha:010", "alpha:011", "alpha:012", "beta:010", "alpha:003")]
        related = get_related_data([issue.id for issue in version_targets])
        version_rows = [row for row in (create_issue_version(issue, related) for issue in version_targets) if row]
        for row in version_rows:
            row.id = stable(f"issue-version:{row.issue_id}")
        IssueVersion.objects.bulk_create(version_rows)
        self.remember("issue_versions")

        # Description versions: the `sync_issue_description_version` task
        # builds the same rows. (The model's `log_issue_description_version`
        # passes `owned_by_id=user` — a User object — and is caught by its own
        # try/except at plane/db/models/issue.py:806, so it never writes.)
        from plane.bgtasks.issue_description_version_sync import (
            get_owner_id as description_owner_id,
        )
        from plane.db.models import IssueDescriptionVersion

        description_rows = []
        for issue, revision in (
            (issues["alpha:010"], 0),
            (issues["alpha:010"], 1),
            (issues["alpha:011"], 0),
            (issues["alpha:050"], 0),
        ):
            description_rows.append(
                IssueDescriptionVersion(
                    id=stable(f"issue-description-version:{issue.id}:{revision}"),
                    workspace_id=issue.workspace_id,
                    project_id=issue.project_id,
                    created_by_id=issue.created_by_id,
                    updated_by_id=issue.updated_by_id,
                    owned_by_id=description_owner_id(issue),
                    last_saved_at=self.base,
                    issue_id=issue.id,
                    description_binary=issue.description_binary,
                    description_html=issue.description_html,
                    description_stripped=issue.description_stripped,
                    description_json=issue.description_json,
                )
            )
        IssueDescriptionVersion.objects.bulk_create(description_rows)
        self.remember("issue_description_versions")

        version_ids = list(
            IssueVersion.objects.order_by("created_at").values_list("id", flat=True)[:5]
        )
        for index, version_id in enumerate(version_ids):
            self.var(f"issue_version_{index}", version_id)
        description_version_ids = list(
            IssueDescriptionVersion.objects.order_by("created_at").values_list("id", flat=True)[:5]
        )
        for index, version_id in enumerate(description_version_ids):
            self.var(f"issue_description_version_{index}", version_id)

        from plane.db.models import IssueActivity

        activity_rows = list(
            IssueActivity.objects.filter(issue=issues["alpha:010"]).order_by("id").values_list("id", flat=True)
        )
        for index, activity_id in enumerate(activity_rows[:3]):
            self.var(f"issue_activity_{index}", activity_id)
    # -- intake ------------------------------------------------------------

    def create_intake(self) -> None:
        from plane.db.models import Intake, IntakeIssue, Issue

        users = self.objects["users"]
        projects = self.objects["projects"]
        issues: dict[str, Issue] = self.objects["issues"]
        base = self.base

        intakes = {}
        for project_key, name in (("alpha", "Alpha Intake"), ("beta", "Beta Intake")):
            project = projects[project_key]
            intake = Intake.objects.create(
                id=stable(f"intake:{project_key}"),
                project=project,
                workspace=project.workspace,
                name=name,
                description=f"{name} description",
                is_default=True,
                view_props={"filters": {}, "display_filters": {"layout": "list"}},
                logo_props={"in_use": "emoji", "emoji": {"value": "📥"}},
                created_by_id=users["admin"].id,
            )
            intakes[project_key] = intake
            self.var(f"intake_{project_key}", intake.id)
        self.remember("intakes")
        self.objects["intakes"] = intakes

        specimens = [
            ("alpha", "pending", -2, None, None),
            ("alpha", "rejected", -1, None, None),
            ("alpha", "snoozed", 0, 7, None),
            ("alpha", "accepted", 1, None, None),
            ("alpha", "duplicate", 2, None, "alpha:020"),
            ("beta", "pending", -2, None, None),
            ("beta", "snoozed", 0, 3, None),
        ]
        intake_issues = {}
        for index, (project_key, status_name, status, snooze_days, duplicate_key) in enumerate(specimens):
            issue = issues[f"{project_key}:{100 + index:03d}"]
            row = IntakeIssue.objects.create(
                id=stable(f"intake-issue:{project_key}:{status_name}"),
                project=issue.project,
                workspace=issue.workspace,
                intake=intakes[project_key],
                issue=issue,
                status=status,
                snoozed_till=(base + timedelta(days=snooze_days)) if snooze_days is not None else None,
                duplicate_to=issues[duplicate_key] if duplicate_key else None,
                source="IN_APP",
                source_email="reporter@example.test" if index % 2 else None,
                extra={"via": "fixture"},
                created_by_id=users["member"].id,
            )
            intake_issues[(project_key, status_name)] = row
            self.var(f"intake_issue_{project_key}_{status_name}", row.id)
            # The intake endpoints serialise Issues (`IntakeIssueViewSet`
            # returns Issue rows), so the detail route's `{pk}` is an Issue id.
            self.var(f"intake_issue_{project_key}_{status_name}_issue", issue.id)
        self.remember("intake_issues")
        self.objects["intake_issues"] = intake_issues

        IntakeIssue.objects.filter(pk=intake_issues[("alpha", "rejected")].pk).update(
            deleted_at=base - timedelta(days=3)
        )

    # -- drafts ------------------------------------------------------------

    def create_drafts(self) -> None:
        from plane.db.models import (
            DraftIssue,
            DraftIssueAssignee,
            DraftIssueCycle,
            DraftIssueLabel,
            DraftIssueModule,
        )

        users = self.objects["users"]
        projects = self.objects["projects"]
        states = self.objects["states"]
        labels = self.objects["labels"]
        modules = self.objects["modules"]
        cycles = self.objects["cycles"]
        base = self.base

        drafts = {}
        specs = [
            # key, project, name, state, priority, owner
            ("admin_alpha", "alpha", "Draft: offline mode", "Backlog", "low", "admin"),
            ("admin_beta", "beta", "Draft: widget refresh", "Todo", "medium", "admin"),
            ("member_alpha", "alpha", "Draft: bulk edit labels", "Todo", "high", "member"),
            ("member_beta", "beta", "Draft: crash on rotation", "In Progress", "urgent", "member"),
        ]
        for key, project_key, name, state_name, priority, owner in specs:
            project = projects[project_key]
            with self.acting_as(owner):
                draft = DraftIssue(
                    id=stable(f"draft-issue:{key}"),
                    workspace=project.workspace,
                    project=project,
                    state=states[(project_key, state_name)],
                    name=name,
                    priority=priority,
                    description_html=f"<p>{name}</p>",
                    description_json={"type": "doc", "content": [{"type": "paragraph"}]},
                    start_date=(base - timedelta(days=3)).date(),
                    target_date=(base + timedelta(days=11)).date(),
                )
                draft.save()
            drafts[key] = draft
            self.var(f"draft_{key}", draft.id)
            self.var(f"draft_{key}_name", name)
            self.remember("draft_issues")

            assignee = DraftIssueAssignee.objects.create(
                id=stable(f"draft-assignee:{key}"),
                workspace=project.workspace,
                project=project,
                draft_issue=draft,
                assignee=users["member" if owner == "admin" else "admin"],
                created_by_id=users[owner].id,
            )
            self.remember("draft_issue_assignees")
            self.var(f"draft_assignee_{key}", assignee.id)
            label_key = "alpha:bug" if project_key == "alpha" else "beta:mobile_bug"
            label = DraftIssueLabel.objects.create(
                id=stable(f"draft-label:{key}"),
                workspace=project.workspace,
                project=project,
                draft_issue=draft,
                label=labels[label_key],
                created_by_id=users[owner].id,
            )
            self.remember("draft_issue_labels")
            self.var(f"draft_label_{key}", label.id)
            module_key = "alpha:in_progress" if project_key == "alpha" else "beta:in_progress"
            module = DraftIssueModule.objects.create(
                id=stable(f"draft-module:{key}"),
                workspace=project.workspace,
                project=project,
                draft_issue=draft,
                module=modules[module_key],
                created_by_id=users[owner].id,
            )
            self.remember("draft_issue_modules")
            self.var(f"draft_module_{key}", module.id)
            cycle_key = "alpha:current" if project_key == "alpha" else "beta:current"
            cycle = DraftIssueCycle.objects.create(
                id=stable(f"draft-cycle:{key}"),
                workspace=project.workspace,
                project=project,
                draft_issue=draft,
                cycle=cycles[cycle_key],
                created_by_id=users[owner].id,
            )
            self.remember("draft_issue_cycles")
            self.var(f"draft_cycle_{key}", cycle.id)
        self.objects["drafts"] = drafts

    # -- pages -------------------------------------------------------------
    def create_pages(self) -> None:
        from plane.db.models import Label, Page, PageLabel, PageLog, PageVersion, ProjectPage

        users = self.objects["users"]
        projects = self.objects["projects"]
        labels = self.objects["labels"]
        ws = self.objects["workspaces"]["ws"]
        base = self.base

        pages: dict[str, Page] = {}
        page_specs = [
            # key, name, access, locked, archived, parent, global
            ("public", "Workspace Handbook", 0, False, None, None, False),
            ("private", "Leadership Notes", 1, False, None, None, False),
            ("locked", "Release Checklist", 0, True, None, None, False),
            ("archived", "Old Roadmap", 0, False, -30, None, False),
            ("parent", "Engineering Home", 0, False, None, None, False),
            ("child", "Architecture", 1, False, None, "parent", False),
            ("grandchild", "Data Model", 0, False, None, "child", True),
            ("project", "Alpha Project Page", 0, False, None, None, False),
        ]
        for key, name, access, locked, archived_days, parent_key, is_global in page_specs:
            page = Page(
                id=stable(f"page:{key}"),
                workspace=ws,
                name=name,
                description_html=f"<p>{name} body</p>",
                description_json={"type": "doc", "content": [{"type": "paragraph"}]},
                # `PagesDescriptionViewSet.retrieve` streams this column as an
                # octet-stream (plane/app/views/page/base.py:506-516).
                description_binary=b"fixture page binary description",
                owned_by=users["admin"] if key != "private" else users["member"],
                access=access,
                color="#F59E0B",
                parent=pages.get(parent_key) if parent_key else None,
                archived_at=(base - timedelta(days=archived_days)).date() if archived_days is not None else None,
                is_locked=locked,
                view_props={"full_width": key == "public"},
                logo_props={"in_use": "emoji", "emoji": {"value": "📄"}},
                is_global=is_global,
                sort_order=65535 + (len(pages) * 10000),
                created_by_id=users["admin"].id,
            )
            page.save()
            pages[key] = page
            self.var(f"page_{key}", page.id)
            self.var(f"page_{key}_name", name)
        self.remember("pages")
        self.objects["pages"] = pages

        # Labels on pages (the M2M through table).
        for index, label_key in enumerate(("cross_cutting", "design_system")):
            row = PageLabel.objects.create(
                id=stable(f"page-label:{index}"),
                workspace=ws,
                page=pages["public"],
                label=labels[label_key],
                created_by_id=users["admin"].id,
            )
            self.remember("page_labels")
            self.var(f"page_label_{index}", row.id)

        # Project pages, including the M2M that makes a page visible to a
        # project.
        for index, (project_key, page_key) in enumerate((("alpha", "project"), ("beta", "public"), ("alpha", "child"))):
            row = ProjectPage.objects.create(
                id=stable(f"project-page:{project_key}:{page_key}"),
                workspace=projects[project_key].workspace,
                project=projects[project_key],
                page=pages[page_key],
                created_by_id=users["admin"].id,
            )
            self.remember("project_pages")
            self.var(f"project_page_{project_key}_{page_key}", row.id)

        # Page versions (newest 20 are returned; 3 are enough to exercise the
        # list and detail routes).
        for page_key in ("public", "child"):
            for revision in range(3):
                version = PageVersion.objects.create(
                    id=stable(f"page-version:{page_key}:{revision}"),
                    workspace=ws,
                    page=pages[page_key],
                    owned_by=users["admin"],
                    description_html=f"<p>Revision {revision} of {pages[page_key].name}</p>",
                    description_json={"type": "doc", "content": [{"type": "paragraph"}]},
                    sub_pages_data={"count": revision},
                )
                self.remember("page_versions")
                self.var(f"page_version_{page_key}_{revision}", version.id)

        # Page logs: the rows the page transaction task writes.
        log_specs = [
            ("public", "issue", "ISSUE", str(self.v["issue_alpha"])),
            ("public", "cycle", "CYCLE", str(self.v["cycle_alpha_current"])),
            ("child", "user_mention", "USER_MENTION", str(users["member"].id)),
        ]
        for index, (page_key, kind, entity_type, entity_id) in enumerate(log_specs):
            log = PageLog.objects.create(
                id=stable(f"page-log:{index}"),
                transaction=stable(f"page-log-transaction:{index}"),
                page=pages[page_key],
                workspace=ws,
                entity_identifier=entity_id,
                entity_name=kind,
                entity_type=entity_type,
                created_by_id=users["admin"].id,
            )
            self.remember("page_logs")
            self.var(f"page_log_{index}", log.id)

    # -- views -------------------------------------------------------------

    def create_views(self) -> None:
        from plane.db.models import IssueView

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        base = self.base

        views: dict[str, IssueView] = {}
        specs = [
            ("ws_public", None, "Workspace Triage", 1, False, None),
            ("ws_private", None, "Workspace Private", 0, True, None),
            ("alpha_high", "alpha", "Alpha High Priority", 1, False, None),
            ("alpha_started", "alpha", "Alpha Started", 1, False, None),
            ("alpha_archived", "alpha", "Alpha Archived View", 1, False, -15),
            ("beta_bugs", "beta", "Beta Bugs", 1, False, None),
        ]
        filters = {
            "ws_public": {"priority": "high"},
            "ws_private": {},
            "alpha_high": {"priority": "high", "state_group": "started"},
            "alpha_started": {"state_group": "started"},
            "alpha_archived": {"priority": "low"},
            "beta_bugs": {"labels": None},
        }
        for key, project_key, name, access, locked, archived_days in specs:
            view = IssueView(
                id=stable(f"view:{key}"),
                workspace=ws,
                project=projects[project_key] if project_key else None,
                name=name,
                description=f"{name} description",
                filters=filters[key],
                query={},
                display_filters={"layout": "list", "group_by": "state", "order_by": "-created_at"},
                display_properties={"assignee": True, "cycle": True, "modules": False},
                rich_filters={"and": []},
                access=access,
                logo_props={"in_use": "icon", "icon": {"name": "list"}},
                owned_by=users["admin"],
                is_locked=locked,
                archived_at=(base - timedelta(days=archived_days)) if archived_days is not None else None,
                created_by_id=users["admin"].id,
            )
            view.save()
            views[key] = view
            self.var(f"view_{key}", view.id)
            self.var(f"view_{key}_name", name)
        self.remember("issue_views")
        self.objects["views"] = views

    # -- favourites --------------------------------------------------------

    def create_favorites(self) -> None:
        from plane.db.models import UserFavorite

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        cycles = self.objects["cycles"]
        modules = self.objects["modules"]
        views = self.objects["views"]
        pages = self.objects["pages"]
        intakes = self.objects["intakes"]

        folder = UserFavorite.objects.create(
            id=stable("favorite:folder"),
            workspace=ws,
            user=users["admin"],
            entity_type="folder",
            entity_identifier=stable("favorite-folder-id"),
            name="Pinned",
            is_folder=True,
            sequence=65535,
            created_by_id=users["admin"].id,
        )
        self.remember("user_favorites")
        self.var("favorite_folder", folder.id)

        entries = [
            ("project", projects["alpha"], projects["alpha"].name, True),
            ("cycle", cycles["alpha:current"], "Alpha Sprint 2", False),
            ("module", modules["alpha:in_progress"], "Alpha In Progress", False),
            ("view", views["alpha_high"], "Alpha High Priority", False),
            ("page", pages["public"], "Workspace Handbook", False),
            ("intake", intakes["alpha"], "Alpha Intake", False),
            ("issue", None, "Favorite issue", False),
            ("project", projects["beta"], projects["beta"].name, False),
        ]
        for index, (entity_type, obj, name, in_folder) in enumerate(entries):
            identifier = obj.id if obj is not None else self.v["issue_alpha"]
            favorite = UserFavorite.objects.create(
                id=stable(f"favorite:{entity_type}:{index}"),
                workspace=ws,
                user=users["admin"] if index % 2 == 0 else users["member"],
                entity_type=entity_type,
                entity_identifier=identifier,
                name=name,
                parent=folder if in_folder else None,
                created_by_id=users["admin"].id,
            )
            self.remember("user_favorites")
            self.var(f"favorite_{entity_type}", favorite.id)
        # A page favourite **with** a project. `WorkspaceFavoriteEndpoint.get`
        # drops page favourites without one (`favorite.py:27-35`), and the
        # entity gate then needs an active `project_pages` link to that project
        # (`serializer/favorite.py`); `page_project` has one, so this is the only
        # row that reaches the page branch (see `workspaces-extra.yaml`).
        page_favorite = UserFavorite.objects.create(
            id=stable("favorite:page:with-project"),
            workspace=ws,
            user=users["member"],
            entity_type="page",
            entity_identifier=pages["project"].id,
            name=pages["project"].name,
            project=projects["alpha"],
            sequence=65535,
            created_by_id=users["admin"].id,
        )
        self.remember("user_favorites")
        self.var("favorite_page_with_project", page_favorite.id)

        # A soft-deleted favourite.
        UserFavorite.objects.filter(entity_type="issue").update(deleted_at=self.base - timedelta(days=9))
        self.objects["favorites"] = {"folder": folder}

    # -- quick links -------------------------------------------------------

    def create_quick_links(self) -> None:
        from plane.db.models import WorkspaceUserLink

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        specs = [
            ("admin", "Design docs", "https://docs.example.test/design"),
            ("admin", "Status page", "https://status.example.test/"),
            ("member", "Runbook", "https://docs.example.test/runbook"),
        ]
        for index, (owner, title, url) in enumerate(specs):
            link = WorkspaceUserLink.objects.create(
                id=stable(f"quick-link:{index}"),
                workspace=ws,
                project=projects["alpha"] if index == 0 else None,
                title=title,
                url=url,
                metadata={"title": title, "favicon": "https://docs.example.test/favicon.ico"},
                owner=users[owner],
                created_by_id=users[owner].id,
            )
            self.remember("workspace_user_links")
            self.var(f"quick_link_{index}", link.id)

    # -- stickies ----------------------------------------------------------

    def create_stickies(self) -> None:
        from plane.db.models import Sticky

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        specs = [
            ("admin", "Release mantra", "#FDE68A", "#111827"),
            ("admin", "Known issues", "#BFDBFE", "#1E3A8A"),
            ("member", "Interview notes", "#BBF7D0", "#065F46"),
        ]
        for index, (owner, name, background, colour) in enumerate(specs):
            sticky = Sticky.objects.create(
                id=stable(f"sticky:{index}"),
                workspace=ws,
                owner=users[owner],
                name=name,
                description={"type": "doc", "content": [{"type": "paragraph"}]},
                description_html=f"<p>{name}</p>",
                color=colour,
                background_color=background,
                logo_props={"in_use": "emoji", "emoji": {"value": "🗒️"}},
                created_by_id=users[owner].id,
            )
            self.remember("stickies")
            self.var(f"sticky_{index}", sticky.id)

    # -- recent visits -----------------------------------------------------

    def create_recent_visits(self) -> None:
        from plane.db.models import UserRecentVisit

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        # Lower case: the views enqueue `entity_name="project"`/`"issue"`/`"page"`
        # (plane/app/views/page/base.py:234, ...), and the read endpoint filters
        # `entity_name__in=["issue", "page", "project"]`
        # (plane/app/views/workspace/recent_visit.py:33). The uppercase
        # `EntityNameEnum` values are never written.
        entries = [
            ("admin", "project", projects["alpha"].id),
            ("admin", "project", projects["beta"].id),
            ("admin", "issue", self.v["issue_alpha"]),
            ("member", "project", projects["alpha"].id),
            ("member", "page", self.v["page_public"]),
            ("member", "view", self.v["view_alpha_high"]),
            ("guest", "project", projects["epsilon"].id),
        ]
        for index, (user_name, entity_name, identifier) in enumerate(entries):
            visit = UserRecentVisit.objects.create(
                id=stable(f"recent-visit:{index}"),
                workspace=ws,
                user=users[user_name],
                entity_name=entity_name,
                entity_identifier=identifier,
                created_by_id=users[user_name].id,
            )
            self.remember("user_recent_visits")
            self.var(f"recent_visit_{index}", visit.id)

    # -- user/workspace/project properties ---------------------------------

    def create_user_properties(self) -> None:
        from plane.db.models import (
            ProjectUserProperty,
            WorkspaceHomePreference,
            WorkspaceUserPreference,
            WorkspaceUserProperties,
        )

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]

        for user_name in ("admin", "member", "guest"):
            props = WorkspaceUserProperties.objects.create(
                id=stable(f"workspace-user-properties:{user_name}"),
                workspace=ws,
                user=users[user_name],
                filters={"priority": "high"},
                display_filters={"display_filters": {"layout": "kanban", "group_by": "state"}},
                display_properties={"display_properties": {"assignee": False, "state": True}},
                rich_filters={"and": []},
                navigation_control_preference="TABBED" if user_name == "admin" else "ACCORDION",
                created_by_id=users[user_name].id,
            )
            self.remember("workspace_user_properties")
            self.var(f"workspace_user_properties_{user_name}", props.id)

        # Project display properties: ProjectMember.save already created one
        # row per (project, user); give the flagship ones non-default content
        # from the read endpoint's point of view.
        for project_key in ("alpha", "beta"):
            for user_name in ("admin", "member"):
                property_row = (
                    ProjectUserProperty.objects.filter(
                        project=projects[project_key], user=users[user_name]
                    ).first()
                )
                if property_row is None:
                    continue
                property_row.display_properties = {
                    "assignee": True,
                    "attachment_count": False,
                    "created_on": True,
                    "due_date": True,
                    "estimate": True,
                    "key": True,
                    "labels": True,
                    "link": True,
                    "priority": True,
                    "start_date": True,
                    "state": True,
                    "sub_issue_count": True,
                    "updated_on": True,
                }
                property_row.display_filters = {
                    "group_by": "state",
                    "order_by": "-created_at",
                    "type": None,
                    "sub_issue": True,
                    "show_empty_groups": True,
                    "layout": "list",
                    "calendar_date_range": "",
                }
                property_row.filters = {"priority": "high"}
                property_row.save()
                self.remember("project_user_properties")
                self.var(f"project_user_property_{project_key}_{user_name}", property_row.id)

        for index, key in enumerate(("quick_links", "recents", "my_stickies", "new_at_plane", "quick_tutorial")):
            preference = WorkspaceHomePreference.objects.create(
                id=stable(f"workspace-home-preference:{index}"),
                workspace=ws,
                user=users["admin"],
                key=key,
                is_enabled=index % 2 == 0,
                config={"visible": True, "order": index},
                sort_order=65535 - index * 1000,
                created_by_id=users["admin"].id,
            )
            self.remember("workspace_home_preferences")
            self.var(f"home_preference_{key.lower()}", preference.id)

        for index, key in enumerate(("views", "active_cycles", "analytics", "drafts", "archives", "stickies")):
            preference = WorkspaceUserPreference.objects.create(
                id=stable(f"workspace-user-preference:{index}"),
                workspace=ws,
                user=users["admin"],
                key=key,
                is_pinned=index % 2 == 0,
                sort_order=65535 - index * 1000,
                created_by_id=users["admin"].id,
            )
            self.remember("workspace_user_preferences")
            self.var(f"user_preference_{key}", preference.id)

    # -- webhooks ----------------------------------------------------------

    def create_workspace_themes(self) -> None:
        from plane.db.models import WorkspaceTheme

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        themes = [
            ("Midnight", "#0F172A", "#1E293B"),
            ("Sunrise", "#FEF3C7", "#F59E0B"),
        ]
        for index, (name, background, accent) in enumerate(themes):
            theme = WorkspaceTheme.objects.create(
                id=stable(f"workspace-theme:{index}"),
                workspace=ws,
                name=name,
                actor=users["admin"],
                colors={"background": background, "accent": accent, "text": "#111827"},
                created_by_id=users["admin"].id,
            )
            self.remember("workspace_themes")
            self.var(f"workspace_theme_{index}", theme.id)

    def create_accounts(self) -> None:
        from plane.db.models import Account

        users = self.objects["users"]
        accounts = [
            ("admin", "google", "google-admin-account"),
            ("member", "github", "github-member-account"),
        ]
        for index, (user_name, provider, provider_account_id) in enumerate(accounts):
            account = Account.objects.create(
                id=stable(f"account:{index}"),
                user=users[user_name],
                provider=provider,
                provider_account_id=provider_account_id,
                access_token=f"fixture-access-token-{index}",
                refresh_token=f"fixture-refresh-token-{index}",
                last_connected_at=self.base - timedelta(days=200 - index),
                id_token=f"fixture-id-token-{index}",
                metadata={"login": provider_account_id},
            )
            self.remember("accounts")
            self.var(f"account_{user_name}", account.id)

    # -- webhooks ----------------------------------------------------------

    def create_webhooks(self) -> None:
        from plane.db.models import Webhook, WebhookLog
        from plane.db.models.webhook import ProjectWebhook

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]

        webhooks = {}
        specs = [
            ("active", "https://hooks.example.test/active", True, True, False, False, False, False),
            ("inactive", "https://hooks.example.test/inactive", False, True, True, True, False, False),
            ("internal", "https://api.example.test/internal", True, False, False, False, False, True),
        ]
        for key, url, active, project_events, module_events, cycle_events, comment_events, internal in specs:
            webhook = Webhook.objects.create(
                id=stable(f"webhook:{key}"),
                workspace=ws,
                url=url,
                is_active=active,
                secret_key=f"plane_wh_{stable_hex(f'webhook-secret:{key}')}",
                project=project_events,
                issue=True,
                module=module_events,
                cycle=cycle_events,
                issue_comment=comment_events,
                is_internal=internal,
                version="v1",
                created_by_id=users["admin"].id,
            )
            webhooks[key] = webhook
            self.var(f"webhook_{key}", webhook.id)
        self.remember("webhooks")

        for project_key in ("alpha", "beta"):
            row = ProjectWebhook.objects.create(
                id=stable(f"project-webhook:{project_key}"),
                project=projects[project_key],
                workspace=projects[project_key].workspace,
                webhook=webhooks["active"],
                created_by_id=users["admin"].id,
            )
            self.remember("project_webhooks")
            self.var(f"project_webhook_{project_key}", row.id)

        logs = [
            ("active", "issue", "POST", "200", 0),
            ("active", "issue_comment", "POST", "500", 2),
            ("active", "project", "POST", "200", 0),
            ("inactive", "cycle", "POST", "404", 1),
        ]
        for index, (webhook_key, event_type, method, response_status, retry_count) in enumerate(logs):
            log = WebhookLog.objects.create(
                id=stable(f"webhook-log:{index}"),
                workspace=ws,
                webhook=webhooks[webhook_key].id,
                event_type=event_type,
                request_method=method,
                request_headers='{"content-type": "application/json"}',
                request_body=json.dumps({"event": event_type, "action": "created"}),
                response_status=response_status,
                response_headers='{"content-type": "application/json"}',
                response_body='{"ok": true}' if response_status == "200" else '{"error": "failed"}',
                retry_count=retry_count,
                created_by_id=users["admin"].id,
            )
            self.remember("webhook_logs")
            self.var(f"webhook_log_{index}", log.id)
        self.var("webhook_log_latest", WebhookLog.objects.filter(webhook=webhooks["active"].id).order_by("created_at").first().id)

    # -- API tokens --------------------------------------------------------

    def create_api_tokens(self) -> None:
        from plane.db.models import APIToken

        users = self.objects["users"]
        ws = self.objects["workspaces"]["ws"]
        base = self.base

        specs = [
            # 64 hex characters: `normalize.yaml` types every JSON `token` as
            # `api_token` (^[0-9a-f]{64}$), and `APIKeyAuthentication` looks the
            # value up verbatim, so the `plane_api_` default prefix is not
            # required. Deterministic and accepted by the v1 routes.
            ("active", "admin", "a" * 62 + "aa", None, True),
            ("member", "member", "b" * 62 + "bb", None, True),
            ("expired", "admin", "c" * 62 + "cc", -30, True),
            ("inactive", "admin", "d" * 62 + "dd", None, False),
            ("workspace", "member", "e" * 62 + "ee", None, True),
            ("bot", "api_bot", "f" * 62 + "ff", None, True),
            # Additional tokens for the same users: the v1 throttle keys on the
            # token, and one read scenario issues more than 60 requests per
            # token per minute.
            ("admin2", "admin", "1" * 62 + "11", None, True),
            ("admin3", "admin", "2" * 62 + "22", None, True),
            ("member2", "member", "3" * 62 + "33", None, True),
            ("member3", "member", "4" * 62 + "44", None, True),
        ]
        for key, user_name, token, expired_days, active in specs:
            row = APIToken.objects.create(
                id=stable(f"api-token:{key}"),
                label=f"fixture-{key}",
                description=f"{key} token",
                token=token,
                user=users[user_name],
                user_type=1 if user_name == "api_bot" else 0,
                workspace=ws if key in {"workspace", "active"} else None,
                expired_at=(base + timedelta(days=expired_days)) if expired_days is not None else None,
                is_active=active,
                last_used=base - timedelta(days=2) if key == "active" else None,
                created_by_id=users[user_name].id,
            )
            self.remember("api_tokens")
            self.var(f"api_token_{key}", row.token)
            self.var(f"api_token_{key}_id", row.id)

    # -- exporters and analytics -------------------------------------------

    def create_exporters(self) -> None:
        from plane.db.models import AnalyticView, ExporterHistory

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        base = self.base

        specs = [
            ("queued", "queued", "json", None, None),
            ("processing", "processing", "csv", None, None),
            ("completed", "completed", "xlsx", "https://exports.example.test/alpha.xlsx", None),
            ("failed", "failed", "csv", None, "Export timed out"),
        ]
        for index, (key, status, provider, url, reason) in enumerate(specs):
            exporter = ExporterHistory.objects.create(
                id=stable(f"exporter:{key}"),
                name=f"Alpha export {key}",
                type="issue_exports",
                workspace=ws,
                project=[projects["alpha"].id],
                provider=provider,
                status=status,
                reason=reason or "",
                key=f"exports/{key}.{provider}",
                url=url,
                token=stable_hex(f"exporter-token:{key}") + stable_hex(f"exporter-token2:{key}"),
                initiated_by=users["admin"],
                filters={"project": [str(projects["alpha"].id)]},
                rich_filters={"and": []},
                created_by_id=users["admin"].id,
            )
            self.remember("exporters")
            self.var(f"exporter_{key}", exporter.id)

        # `SavedAnalyticEndpoint` applies `analytic_view.query` as Django filter
        # kwargs on `Issue.objects` and reads x_axis/y_axis from `query_dict`
        # (plane/app/views/analytic/base.py:199-211).
        analytics = [
            ("default", "Default analytics", {"priority": "high"}, {"x_axis": "priority", "y_axis": "issue_count"}),
            ("custom", "Custom analytics", {"state__group": "started"}, {"x_axis": "state_id", "y_axis": "issue_count"}),
        ]
        for index, (key, name, query, query_dict) in enumerate(analytics):
            view = AnalyticView.objects.create(
                id=stable(f"analytic-view:{key}"),
                workspace=ws,
                name=name,
                description=f"{name} description",
                query=query,
                query_dict=query_dict,
                created_by_id=users["admin"].id,
            )
            self.remember("analytic_views")
            self.var(f"analytic_view_{key}", view.id)

    # -- notifications -----------------------------------------------------

    def create_notifications(self) -> None:
        from plane.db.models import Notification, UserNotificationPreference

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]
        base = self.base

        specs = [
            # key, receiver, entity_name, read_days, archived_days, snoozed_days
            ("unread_issue", "member", "issue", None, None, None),
            ("read_issue", "member", "issue", -2, None, None),
            ("archived_issue", "member", "issue", -5, -3, None),
            ("snoozed_issue", "member", "issue", None, None, 3),
            ("unread_comment", "member", "issue_comment", None, None, None),
            ("unread_project", "member", "project", None, None, None),
            ("unread_cycle", "member", "cycle", None, None, None),
            ("unread_module", "member", "module", None, None, None),
            ("unread_admin", "admin", "issue", None, None, None),
            ("read_admin", "admin", "issue", -1, None, None),
            ("unread_guest", "guest", "issue", None, None, None),
        ]
        notifications = {}
        for index, (key, receiver, entity_name, read_days, archived_days, snoozed_days) in enumerate(specs):
            entity_identifier = {
                "issue": self.v["issue_alpha"],
                "issue_comment": self.v["comment_alpha"],
                "project": projects["alpha"].id,
                "cycle": self.v["cycle_alpha_current"],
                "module": self.v["module_alpha_in_progress"],
            }[entity_name]
            notification = Notification.objects.create(
                id=stable(f"notification:{key}"),
                workspace=ws,
                project=projects["alpha"],
                data={"issue": {"id": self.v["issue_alpha"], "name": "Add CSV export for filtered work items"}},
                entity_identifier=entity_identifier,
                entity_name=entity_name,
                title=f"Notification {key.replace('_', ' ')}",
                message={"actor": "Mia Member"},
                message_html=f"<p>Notification {key}</p>",
                message_stripped=f"Notification {key}",
                sender="in_app:issue_activities:created",
                triggered_by=users["member"],
                receiver=users[receiver],
                read_at=(base + timedelta(days=read_days)) if read_days is not None else None,
                archived_at=(base + timedelta(days=archived_days)) if archived_days is not None else None,
                snoozed_till=(base + timedelta(days=snoozed_days)) if snoozed_days is not None else None,
                created_by_id=users["member"].id,
            )
            notifications[key] = notification
            self.var(f"notification_{key}", notification.id)
        self.remember("notifications")
        self.objects["notifications"] = notifications

        # The `post_save` signal on User already created one preference per
        # user. Add workspace/project-scoped overrides, which is what the
        # notification-preferences serializer writes.
        signal_preferences = list(UserNotificationPreference.objects.order_by("created_at"))
        for index, preference in enumerate(signal_preferences):
            self.var(f"notification_preference_{index}", preference.id)

        scoped = UserNotificationPreference.objects.create(
            id=stable("notification-preference:workspace:member"),
            user=users["member"],
            workspace=ws,
            project=projects["alpha"],
            property_change=False,
            state_change=True,
            comment=True,
            mention=False,
            issue_completed=False,
            created_by_id=users["member"].id,
        )
        self.remember("user_notification_preferences")
        self.var("notification_preference_workspace", scoped.id)

        # One user opts out of everything, so a preference read has variety.
        UserNotificationPreference.objects.filter(user=users["guest"]).update(
            property_change=False,
            state_change=False,
            comment=False,
            mention=False,
            issue_completed=False,
        )

        # The signal-created preference rows have no id (S01, user.py:298-312);
        # re-key everything so two builds agree.
        self.renumber(
            UserNotificationPreference,
            "notification-preference",
            ("user_id", "workspace_id", "project_id"),
        )

    # -- deploy boards (published project) ---------------------------------

    def create_deploy_boards(self) -> None:
        from plane.db.models import DeployBoard
        from plane.db.models.project import ProjectDeployBoard

        users = self.objects["users"]
        projects = self.objects["projects"]
        ws = self.objects["workspaces"]["ws"]

        board = DeployBoard.objects.create(
            id=stable("deploy-board:alpha"),
            workspace=ws,
            project=projects["alpha"],
            entity_identifier=projects["alpha"].id,
            entity_name="project",
            anchor="fixture-anchor-alpha",
            is_comments_enabled=True,
            is_reactions_enabled=True,
            is_votes_enabled=True,
            is_activity_enabled=True,
            # The public intake endpoints 400 when the board has no intake
            # (plane/space/views/intake.py:58-63).
            intake=self.objects["intakes"]["alpha"],
            view_props={"show_only_issues": True},
            created_by_id=users["admin"].id,
        )
        self.remember("deploy_boards")
        self.var("anchor_alpha", board.anchor)
        self.var("deploy_board_alpha", board.id)

        board_beta = DeployBoard.objects.create(
            id=stable("deploy-board:beta"),
            workspace=ws,
            project=projects["beta"],
            entity_identifier=projects["beta"].id,
            entity_name="project",
            anchor="fixture-anchor-beta",
            is_comments_enabled=False,
            is_reactions_enabled=False,
            is_votes_enabled=False,
            created_by_id=users["admin"].id,
        )
        self.remember("deploy_boards")
        self.var("anchor_beta", board_beta.anchor)
        self.var("deploy_board_beta", board_beta.id)

        legacy = ProjectDeployBoard.objects.create(
            id=stable("project-deploy-board:alpha"),
            project=projects["alpha"],
            workspace=ws,
            anchor="fixture-project-anchor-alpha",
            views={"list": True, "kanban": True, "calendar": False, "gantt": False, "spreadsheet": False},
            created_by_id=users["admin"].id,
        )
        self.remember("project_deploy_boards")
        self.var("project_deploy_board_anchor", legacy.anchor)
        self.var("project_deploy_board", legacy.id)

    # -- instance admin ----------------------------------------------------

    def create_instance_admins(self) -> None:
        from plane.license.models import Instance, InstanceAdmin

        users = self.objects["users"]
        instance = Instance.objects.first()
        for user_name, role in (("admin", 20), ("member", 5)):
            row = InstanceAdmin.objects.create(
                id=stable(f"instance-admin:{user_name}"),
                instance=instance,
                user=users[user_name],
                role=role,
                created_by_id=users["admin"].id,
            )
            self.remember("instance_admins")
            self.var(f"instance_admin_{user_name}", row.id)

    # -- timestamps --------------------------------------------------------

    def pin_timestamps(self) -> None:
        """Rewrite every wall-clock column to a value derived from `base`.

        Ordering by `id` makes the assignment stable across builds. Each table
        spreads its rows over ~300 days, so `-created_at` ordering, date
        filters and the activity graph all see a year of history.
        """
        from django.apps import apps as django_apps
        from django.db import connection

        extra_columns = {
            "users": ("date_joined", "last_active", "last_login_time", "last_logout_time", "token_updated_at", "created_at", "updated_at"),
            "user_recent_visits": ("visited_at",),
            "page_versions": ("last_saved_at",),
            "issue_versions": ("last_saved_at",),
            "issue_description_versions": ("last_saved_at",),
            "issue_activities": ("created_at",),
        }
        skip = {"sessions"} | TEMPLATE_TABLES

        models = [
            model
            for model in django_apps.get_models()
            if model._meta.app_label in {"db", "license"} and not model._meta.abstract
        ]
        with connection.cursor() as cursor:
            for model in models:
                table = model._meta.db_table
                if table in skip:
                    continue
                field_names = {field.name for field in model._meta.concrete_fields}
                time_fields = [name for name in extra_columns.get(table, ()) if name in field_names]
                if "created_at" in field_names:
                    for name in ("created_at", "updated_at"):
                        if name in field_names and name not in time_fields:
                            time_fields.append(name)
                if not time_fields:
                    continue
                pk_column = model._meta.pk.column
                cursor.execute(f'SELECT count(*) FROM "{table}"')
                row_count = cursor.fetchone()[0]
                if row_count == 0:
                    continue
                # Spread the table over 300 days, at least one minute apart.
                step = max(60, (300 * 86400) // row_count)
                assignments = []
                params: list = []
                for name in time_fields:
                    if name == "updated_at":
                        assignments.append(f'"{name}" = %s::timestamptz - make_interval(secs => v.rn * {step}) + interval \'15 minutes\'')
                    else:
                        assignments.append(f'"{name}" = %s::timestamptz - make_interval(secs => v.rn * {step})')
                    params.append(self.base)
                cursor.execute(
                    f'WITH v AS (SELECT "{pk_column}" AS pk, row_number() OVER (ORDER BY "{pk_column}") AS rn '
                    f'FROM "{table}") '
                    f'UPDATE "{table}" AS t SET {", ".join(assignments)} FROM v WHERE t."{pk_column}" = v.pk',
                    params,
                )
                self.remember(table)

            # `completed_at` is set by Issue._sync_completed_at to now().
            cursor.execute(
                "UPDATE issues SET completed_at = created_at + interval '2 hours' "
                "WHERE completed_at IS NOT NULL"
            )
            # IssueVersion snapshots copy the issue's completed_at
            # (create_issue_version), which was wall-clock when the snapshot
            # was taken; make it agree with the pinned issue row.
            cursor.execute(
                "UPDATE issue_versions v SET completed_at = i.completed_at "
                "FROM issues i WHERE i.id = v.issue_id AND v.completed_at IS NOT NULL"
            )
            # Explicit anchors: a flagship issue, page and cycle get known
            # instants so the scenario generator can use their calendar dates.
            anchors = [
                ("issues", self.v["issue_alpha"], self.base - timedelta(days=100, minutes=5)),
                ("issues", self.v["issue_alpha_overdue"], self.base - timedelta(days=60, minutes=7)),
                ("issues", self.v["issue_alpha_completed"], self.base - timedelta(days=40, minutes=9)),
                ("issues", self.v["issue_beta"], self.base - timedelta(days=80, minutes=11)),
                ("pages", self.v["page_public"], self.base - timedelta(days=120, minutes=13)),
                ("issue_views", self.v["view_alpha_high"], self.base - timedelta(days=70, minutes=15)),
                ("cycles", self.v["cycle_alpha_current"], self.base - timedelta(days=20, minutes=17)),
            ]
            for table, identifier, instant in anchors:
                cursor.execute(
                    f'UPDATE "{table}" SET created_at = %s, updated_at = %s WHERE id = %s',
                    [instant, instant + timedelta(minutes=15), identifier],
                )
        self.manifest["anchors"] = {
            "issue_alpha_created_day": (self.base - timedelta(days=100)).date().isoformat(),
            "issue_alpha_overdue_created_day": (self.base - timedelta(days=60)).date().isoformat(),
            "issue_beta_created_day": (self.base - timedelta(days=80)).date().isoformat(),
            "issue_created_range": f"{(self.base - timedelta(days=200)).date().isoformat()},{(self.base - timedelta(days=50)).date().isoformat()}",
            "past_date": (self.base - timedelta(days=45)).date().isoformat(),
            "future_date": (self.base + timedelta(days=30)).date().isoformat(),
            "today": self.base.date().isoformat(),
            "yesterday": (self.base - timedelta(days=1)).date().isoformat(),
            "tomorrow": (self.base + timedelta(days=1)).date().isoformat(),
            "cycle_current_start": (self.base - timedelta(days=20)).date().isoformat(),
            "cycle_current_end": (self.base + timedelta(days=20)).date().isoformat(),
        }
        for key, value in self.manifest["anchors"].items():
            self.var(f"date_{key}", value)

    # -- sessions ----------------------------------------------------------

    def create_sessions(self) -> None:
        """Create one signed session per user, plus the instance-admin cookie.

        The payload format is the one `dev/tools/policy/gen_fixture.py` and
        `HTTP_CONTRACT.md` describe: Django `signing.dumps` with the JSON
        serializer and zlib compression. `django.core.signing`'s clock is
        pinned to `base` so the signature (a timestamped value) is reproducible.
        """
        import django.core.signing as signing
        from django.contrib.sessions.serializers import JSONSerializer

        from plane.db.models import Session

        users = self.objects["users"]
        original_time = signing.time.time
        signing.time.time = lambda: self.base.timestamp()
        try:
            session_keys: dict[str, str] = {}
            for name, user in users.items():
                if name in {"inactive", "api_bot"}:
                    continue
                payload = {
                    "_auth_user_id": str(user.id),
                    "_auth_user_backend": "django.contrib.auth.backends.ModelBackend",
                    "_auth_user_hash": user.get_session_auth_hash(),
                    "device_info": {
                        "user_agent": f"fix-fixture-{name}",
                        "ip_address": "203.0.113.42",
                        "domain": "fix.example.test",
                    },
                }
                data = signing.dumps(
                    payload,
                    salt="django.contrib.sessions.SessionStore",
                    serializer=JSONSerializer,
                    compress=True,
                )
                key = (stable_hex(f"session-key:{name}") + "0" * 96)[:128]
                Session.objects.create(
                    session_key=key,
                    session_data=data,
                    expire_date=datetime(2099, 1, 1, tzinfo=dt_timezone.utc),
                    user_id=str(user.id),
                    device_info=payload["device_info"],
                )
                session_keys[name] = key
                self.manifest["sessions"][name] = {
                    "cookie": "session-id",
                    "value": key,
                    "user_id": str(user.id),
                }
                self.var(f"session_{name}", key)
            # The instance-admin cookie is the same store behind a different
            # cookie name (plane/settings/common.py:431; the session middleware
            # selects it when "instances" appears in the path).
            self.manifest["sessions"]["instance_admin"] = {
                "cookie": "admin-session-id",
                "value": session_keys["admin"],
                "user_id": str(users["admin"].id),
            }
            self.var("session_instance_admin", session_keys["admin"])
            self.remember("sessions")
        finally:
            signing.time.time = original_time

    # -- counts ------------------------------------------------------------

    def count_rows(self) -> None:
        from django.db import connection

        counts: dict[str, int] = {}
        with connection.cursor() as cursor:
            for table in sorted(self.tables):
                cursor.execute(f'SELECT count(*) FROM "{table}"')
                counts[table] = cursor.fetchone()[0]
        self.manifest["counts"] = counts


if __name__ == "__main__":
    raise SystemExit(main())
