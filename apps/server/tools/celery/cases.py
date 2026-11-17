# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Representative Celery calls, mirroring the call sites in TASKS.md §5.

This module is pure data plus a few helpers so three tools share one definition:

* ``record.py`` publishes every case to kombu's in-memory transport and writes
  ``golden/messages.json``;
* ``live_publish.py`` publishes one case to a real broker for the live check;
* the Rust golden test builds the same case ids (``plane-jobs``'s
  ``celery::test_cases``) and compares bytes.

``case`` ids must stay in sync with ``test_cases.rs``. Values are the ones the
Python call sites pass, including the UUID-versus-``str`` distinction
(``TASKS.md`` §8.3) and the Python dict insertion order of each call.

Requires Django to be configured already (the ``json.dumps`` helper uses
Django's ``DjangoJSONEncoder``).
"""

from __future__ import annotations

import json
import uuid
from typing import Any

from django.core.serializers.json import DjangoJSONEncoder

# Fixed values, shared with the Rust test table.
WORKSPACE = uuid.UUID("6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60")
USER = uuid.UUID("0d1e2f30-4456-4789-9abc-def012345678")
ISSUE = uuid.UUID("11111111-2222-3333-4444-555555555555")
PROJECT = uuid.UUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
ASSET = uuid.UUID("99999999-8888-7777-6666-555555555555")
MODULE = uuid.UUID("22222222-3333-4444-5555-666666666666")
CYCLE = uuid.UUID("33333333-4444-5555-6666-777777777777")
PAGE = uuid.UUID("44444444-5555-6666-7777-888888888888")
COMMENT = uuid.UUID("55555555-6666-7777-8888-999999999999")
LINK = uuid.UUID("66666666-7777-8888-9999-aaaaaaaaaaaa")
WEBHOOK = uuid.UUID("77777777-8888-9999-aaaa-bbbbbbbbbbbb")
EMAIL_ID = uuid.UUID("88888888-9999-aaaa-bbbb-cccccccccccc")
INVITE = uuid.UUID("99999999-aaaa-bbbb-cccc-dddddddddddd")
DISPATCH = uuid.UUID("12345678-1234-5678-1234-567812345678")

# The publish instant and identity the recorder pins. The Rust encoder takes
# all three as inputs, so only their *values* matter, not how they are derived.
FIXED_NOW = "2026-11-15T17:24:04.144120+00:00"
FIXED_ORIGIN = "gen4242@celery-golden"
FIXED_REPLY_TO = "0f0f0f0f-0000-4000-8000-000000000000"


def case_id(index: int) -> str:
    """A deterministic task id per case (``root_id`` defaults to it)."""
    return f"00000000-0000-4000-8000-{index:012d}"


def jd(value: Any) -> str:
    """``json.dumps(value, cls=DjangoJSONEncoder)`` — the ``str(JSON,DJE)``."""
    return json.dumps(value, cls=DjangoJSONEncoder)


# Convenience aliases for readability in the table below.
S = str

CASES: list[dict[str, Any]] = [
    # -- analytic_plot_export.analytic_export_task
    {
        "case": "analytic_export_task",
        "call_site": "plane/app/views/analytic/base.py:256",
        "task": "plane.bgtasks.analytic_plot_export.analytic_export_task",
        "kwargs": {
            "email": "admin@example.com",
            "data": {
                "project_ids": [str(PROJECT)],
                "filters": {"priority": ["high", "urgent"], "estimate": 2.5},
                "group_by": None,
                "x_axis": "state",
            },
            "slug": "acme",
            "user_id": str(USER),
        },
    },
    # -- copy_s3_object.copy_s3_objects_of_description_and_assets
    {
        "case": "copy_s3_objects",
        "call_site": "plane/app/views/page/base.py:617",
        "task": "plane.bgtasks.copy_s3_object.copy_s3_objects_of_description_and_assets",
        "kwargs": {
            "entity_name": "PAGE",
            "entity_identifier": PAGE,
            "project_id": PROJECT,
            "slug": "acme",
            "user_id": USER,
        },
    },
    # -- deletion_task.soft_delete_related_objects (using a string)
    {
        "case": "soft_delete_related_objects_using",
        "call_site": "plane/db/mixins.py:78",
        "task": "plane.bgtasks.deletion_task.soft_delete_related_objects",
        "args": ["db", "issue", ISSUE],
        "kwargs": {"using": "default"},
    },
    # -- deletion_task.soft_delete_related_objects (using None)
    {
        "case": "soft_delete_related_objects_no_using",
        "call_site": "plane/db/mixins.py:78",
        "task": "plane.bgtasks.deletion_task.soft_delete_related_objects",
        "args": ["db", "project", PROJECT],
        "kwargs": {"using": None},
    },
    # -- email_notification_task.send_email_notification
    {
        "case": "send_email_notification",
        "call_site": "plane/bgtasks/email_notification_task.py:76",
        "task": "plane.bgtasks.email_notification_task.send_email_notification",
        "kwargs": {
            "issue_id": ISSUE,
            "notification_data": {
                str(USER): [
                    {
                        "issue_activity": {
                            "id": str(DISPATCH),
                            "verb": "updated",
                            "field": "priority",
                            "actor": str(USER),
                            "new_value": "high",
                            "old_value": "medium",
                            "old_identifier": None,
                            "new_identifier": None,
                            "activity_time": "2026-11-15 17:24:04",
                        },
                        "issue": {
                            "id": str(ISSUE),
                            "name": "Fix the login redirect loop",
                        },
                    }
                ]
            },
            "receiver_id": str(USER),
            "email_notification_ids": [EMAIL_ID, INVITE],
        },
    },
    # -- export_task.issue_export_task
    {
        "case": "issue_export_task",
        "call_site": "plane/app/views/exporter/base.py:50",
        "task": "plane.bgtasks.export_task.issue_export_task",
        "kwargs": {
            "provider": "csv",
            "workspace_id": WORKSPACE,
            "project_ids": [str(PROJECT), str(PROJECT)],
            "token_id": "exporter-token",
            "multiple": True,
            "slug": "acme",
        },
    },
    # -- forgot_password_task.forgot_password
    {
        "case": "forgot_password",
        "call_site": "plane/authentication/views/app/password_management.py:87",
        "task": "plane.bgtasks.forgot_password_task.forgot_password",
        "args": ["Ada", "ada@example.com", "dXNlcjox", "abc-def", "http://localhost:3000"],
    },
    # -- issue_activities_task.issue_activity: the nine call-site shapes
    {
        "case": "issue_activity_view",
        "call_site": "plane/app/views/issue/base.py:689",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.updated",
            "requested_data": jd(
                {
                    "name": "Fix the login redirect loop",
                    "priority": "high",
                    "assignees": [str(USER)],
                    "target_date": "2026-11-20",
                }
            ),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": jd(
                {
                    "name": "Fix login redirect",
                    "priority": "medium",
                    "assignees": [],
                    "target_date": None,
                }
            ),
            "epoch": 1794764767,
            "notification": True,
            "origin": "http://localhost:3000",
        },
    },
    {
        "case": "issue_activity_bare",
        "call_site": "plane/api/views/issue.py:936",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.deleted",
            "requested_data": jd({"issue_id": str(ISSUE)}),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": jd({"name": "Fix login redirect"}),
            "epoch": 1794764767,
        },
    },
    {
        "case": "issue_activity_view_with_intake_notification",
        "call_site": "plane/app/views/intake/base.py:281",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.created",
            "requested_data": jd({"name": "Inbox item"}),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": None,
            "epoch": 1794764767,
            "notification": True,
            "origin": "http://localhost:3000",
            "intake": str(INVITE),
        },
    },
    {
        "case": "issue_activity_view_with_intake_no_notification",
        "call_site": "plane/app/views/intake/base.py:465",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "intake.activity.created",
            "requested_data": jd({"status": -1}),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": jd({"status": -2}),
            "epoch": 1794764767,
            "notification": False,
            "origin": "http://localhost:3000",
            "intake": str(INVITE),
        },
    },
    {
        "case": "issue_activity_automation",
        "call_site": "plane/bgtasks/issue_automation_task.py:74",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.updated",
            "requested_data": jd({"archived_at": "2026-10-01", "automation": True}),
            "actor_id": str(USER),
            "issue_id": ISSUE,
            "project_id": PROJECT,
            "current_instance": jd({"archived_at": None}),
            "subscriber": False,
            "epoch": 1794764767,
            "notification": True,
        },
    },
    {
        "case": "issue_activity_bulk_date",
        "call_site": "plane/app/views/issue/base.py:1157",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.updated",
            "requested_data": jd({"start_date": "2026-11-20"}),
            "current_instance": jd({"start_date": "2026-11-01"}),
            "issue_id": str(ISSUE),
            "actor_id": str(USER),
            "project_id": str(PROJECT),
            "epoch": 1794764767,
        },
    },
    {
        "case": "issue_activity_with_intake",
        "call_site": "plane/api/views/intake.py:218",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.created",
            "requested_data": jd({"name": "Inbox item"}),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": None,
            "epoch": 1794764767,
            "intake": str(INVITE),
        },
    },
    {
        "case": "issue_activity_with_origin",
        "call_site": "plane/api/views/module.py:563",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "module.activity.deleted",
            "requested_data": jd({"module_id": str(MODULE)}),
            "actor_id": str(USER),
            "issue_id": None,
            "project_id": str(PROJECT),
            "current_instance": jd({"module_name": "Roadmap"}),
            "epoch": 1794764767,
            "origin": "http://localhost:3000",
        },
    },
    {
        "case": "issue_activity_view_with_subscriber",
        "call_site": "plane/app/views/issue/base.py:730",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "issue.activity.deleted",
            "requested_data": jd({"issue_id": str(ISSUE)}),
            "actor_id": str(USER),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "current_instance": {},
            "epoch": 1794764767,
            "notification": True,
            "origin": "http://localhost:3000",
            "subscriber": False,
        },
    },
    {
        "case": "issue_activity_link_created",
        "call_site": "plane/api/views/issue.py:1286",
        "task": "plane.bgtasks.issue_activities_task.issue_activity",
        "kwargs": {
            "type": "link.activity.created",
            "requested_data": jd(
                {"id": str(LINK), "title": "Example", "url": "https://example.com"}
            ),
            "issue_id": str(ISSUE),
            "project_id": str(PROJECT),
            "actor_id": str(USER),
            "current_instance": None,
            "epoch": 1794764767,
        },
    },
    # -- issue_description_version_sync.schedule_issue_description_version
    {
        "case": "schedule_issue_description_version",
        "call_site": "plane/db/management/commands/sync_issue_description_version.py:21",
        "task": "plane.bgtasks.issue_description_version_sync.schedule_issue_description_version",
        "kwargs": {"batch_size": "5000", "countdown": 300},
    },
    # -- issue_description_version_sync.sync_issue_description_version
    {
        "case": "sync_issue_description_version_continue",
        "call_site": "plane/bgtasks/issue_description_version_sync.py:109",
        "task": "plane.bgtasks.issue_description_version_sync.sync_issue_description_version",
        "kwargs": {"batch_size": 5000, "offset": 5000, "countdown": 300},
        "countdown": 300,
    },
    {
        "case": "sync_issue_description_version_start",
        "call_site": "plane/bgtasks/issue_description_version_sync.py:125",
        "task": "plane.bgtasks.issue_description_version_sync.sync_issue_description_version",
        "kwargs": {"batch_size": 5000, "countdown": 300},
    },
    # -- issue_description_version_task.issue_description_version_task
    {
        "case": "issue_description_version_task_create",
        "call_site": "plane/app/views/issue/base.py:484",
        "task": "plane.bgtasks.issue_description_version_task.issue_description_version_task",
        "kwargs": {
            "updated_issue": jd({"name": "New issue", "description_html": "<p>hi</p>"}),
            "issue_id": str(ISSUE),
            "user_id": USER,
            "is_creating": True,
        },
    },
    {
        "case": "issue_description_version_task_update",
        "call_site": "plane/app/views/issue/base.py:710",
        "task": "plane.bgtasks.issue_description_version_task.issue_description_version_task",
        "kwargs": {
            "updated_issue": jd({"name": "Updated issue", "description_html": "<p>bye</p>"}),
            "issue_id": str(ISSUE),
            "user_id": USER,
        },
    },
    # -- issue_version_sync.schedule_issue_version
    {
        "case": "schedule_issue_version",
        "call_site": "plane/db/management/commands/sync_issue_version.py:19",
        "task": "plane.bgtasks.issue_version_sync.schedule_issue_version",
        "kwargs": {"batch_size": "5000", "countdown": 300},
    },
    # -- issue_version_sync.sync_issue_version
    {
        "case": "sync_issue_version_continue",
        "call_site": "plane/bgtasks/issue_version_sync.py:218",
        "task": "plane.bgtasks.issue_version_sync.sync_issue_version",
        "kwargs": {"batch_size": 5000, "offset": 5000, "countdown": 300},
        "countdown": 300,
    },
    {
        "case": "sync_issue_version_start",
        "call_site": "plane/bgtasks/issue_version_sync.py:236",
        "task": "plane.bgtasks.issue_version_sync.sync_issue_version",
        "kwargs": {"batch_size": 5000, "countdown": 300},
    },
    # -- logger_task.process_logs
    {
        "case": "process_logs",
        "call_site": "plane/middleware/logger.py:158",
        "task": "plane.bgtasks.logger_task.process_logs",
        "kwargs": {
            "log_data": {
                "token_identifier": "b3f1c0d2e3a4",
                "path": "/api/v1/workspaces/acme/projects/",
                "method": "GET",
                "query_params": "per_page=20",
                "headers": "{'User-Agent': 'curl/8.7.1'}",
                "body": None,
                "response_body": '{"count": 1}',
                "response_code": 200,
                "ip_address": "203.0.113.7",
                "user_agent": "curl/8.7.1",
            }
        },
    },
    # -- magic_link_code_task.magic_link
    {
        "case": "magic_link",
        "call_site": "plane/authentication/views/app/magic.py:57",
        "task": "plane.bgtasks.magic_link_code_task.magic_link",
        "args": ["ada@example.com", "magic-key", "magic-token"],
    },
    # -- notification_task.notifications (UUID ids)
    {
        "case": "notifications_uuid_ids",
        "call_site": "plane/bgtasks/issue_activities_task.py:1633",
        "task": "plane.bgtasks.notification_task.notifications",
        "kwargs": {
            "type": "issue.activity.updated",
            "issue_id": ISSUE,
            "actor_id": str(USER),
            "project_id": PROJECT,
            "subscriber": True,
            "issue_activities_created": jd(
                [
                    {
                        "id": str(DISPATCH),
                        "verb": "updated",
                        "field": "priority",
                        "actor": str(USER),
                    }
                ]
            ),
            "requested_data": jd({"priority": "high"}),
            "current_instance": jd({"priority": "medium"}),
        },
    },
    # -- notification_task.notifications (None issue, string ids)
    {
        "case": "notifications_string_ids",
        "call_site": "plane/bgtasks/issue_activities_task.py:1633",
        "task": "plane.bgtasks.notification_task.notifications",
        "kwargs": {
            "type": "cycle.activity.created",
            "issue_id": None,
            "actor_id": str(USER),
            "project_id": str(PROJECT),
            "subscriber": False,
            "issue_activities_created": jd([]),
            "requested_data": None,
            "current_instance": jd({"created_cycle_issues": []}),
        },
    },
    # -- page_transaction_task.page_transaction (create: string page id)
    {
        "case": "page_transaction_create",
        "call_site": "plane/app/views/page/base.py:160",
        "task": "plane.bgtasks.page_transaction_task.page_transaction",
        "kwargs": {
            "new_description_html": "<p>Hello 'world' — café 😀 \\ path</p>",
            "old_description_html": None,
            "page_id": str(PAGE),
        },
    },
    # -- page_transaction_task.page_transaction (update: UUID page id)
    {
        "case": "page_transaction_update",
        "call_site": "plane/app/views/page/base.py:558",
        "task": "plane.bgtasks.page_transaction_task.page_transaction",
        "kwargs": {
            "new_description_html": "<p>Hello \"world\"</p>\n<p>Second line</p>",
            "old_description_html": "<p>Hello</p>",
            "page_id": PAGE,
        },
    },
    # -- page_version_task.track_page_version
    {
        "case": "track_page_version",
        "call_site": "plane/app/views/page/base.py:565",
        "task": "plane.bgtasks.page_version_task.track_page_version",
        "kwargs": {
            "page_id": PAGE,
            "existing_instance": jd(
                {"id": str(PAGE), "name": "Doc", "description_html": "<p>Hi</p>"}
            ),
            "user_id": USER,
        },
    },
    # -- project_add_user_email_task.project_add_user_email
    {
        "case": "project_add_user_email",
        "call_site": "plane/app/views/project/member.py:154",
        "task": "plane.bgtasks.project_add_user_email_task.project_add_user_email",
        "args": ["http://localhost:3000", INVITE, USER],
    },
    # -- recent_visited_task.recent_visited_task (project-keyed order)
    {
        "case": "recent_visited_project",
        "call_site": "plane/app/views/project/base.py:247",
        "task": "plane.bgtasks.recent_visited_task.recent_visited_task",
        "kwargs": {
            "slug": "acme",
            "project_id": PROJECT,
            "entity_name": "project",
            "entity_identifier": PROJECT,
            "user_id": USER,
        },
    },
    # -- recent_visited_task.recent_visited_task (entity-keyed order, strings and None)
    {
        "case": "recent_visited_entity",
        "call_site": "plane/app/views/view/base.py:140",
        "task": "plane.bgtasks.recent_visited_task.recent_visited_task",
        "kwargs": {
            "slug": "acme",
            "entity_name": "view",
            "entity_identifier": str(PAGE),
            "user_id": str(USER),
            "project_id": None,
        },
    },
    # -- storage_metadata_task.get_asset_object_metadata (keyword form)
    {
        "case": "get_asset_object_metadata_kwarg",
        "call_site": "plane/app/views/asset/v2.py:219",
        "task": "plane.bgtasks.storage_metadata_task.get_asset_object_metadata",
        "kwargs": {"asset_id": str(ASSET)},
    },
    # -- storage_metadata_task.get_asset_object_metadata (positional form)
    {
        "case": "get_asset_object_metadata_positional",
        "call_site": "plane/app/views/issue/attachment.py:277",
        "task": "plane.bgtasks.storage_metadata_task.get_asset_object_metadata",
        "args": [str(ASSET)],
    },
    # -- user_activation_email_task.user_activation_email
    {
        "case": "user_activation_email",
        "call_site": "plane/authentication/adapter/base.py:248",
        "task": "plane.bgtasks.user_activation_email_task.user_activation_email",
        "args": ["http://localhost:3000", USER],
    },
    # -- user_deactivation_email_task.user_deactivation_email
    {
        "case": "user_deactivation_email",
        "call_site": "plane/app/views/user/base.py:370",
        "task": "plane.bgtasks.user_deactivation_email_task.user_deactivation_email",
        "args": ["http://localhost:3000", USER],
    },
    # -- user_email_update_task.send_email_update_confirmation
    {
        "case": "send_email_update_confirmation",
        "call_site": "plane/app/views/user/base.py:270",
        "task": "plane.bgtasks.user_email_update_task.send_email_update_confirmation",
        "args": ["new@example.com"],
    },
    # -- user_email_update_task.send_email_update_magic_code
    {
        "case": "send_email_update_magic_code",
        "call_site": "plane/app/views/user/base.py:178",
        "task": "plane.bgtasks.user_email_update_task.send_email_update_magic_code",
        "args": ["new@example.com", "123456"],
    },
    # -- webhook_task.model_activity (create)
    {
        "case": "model_activity_create",
        "call_site": "plane/app/views/project/base.py:301",
        "task": "plane.bgtasks.webhook_task.model_activity",
        "kwargs": {
            "model_name": "project",
            "model_id": str(PROJECT),
            "requested_data": {"name": "Apollo", "identifier": "APO"},
            "current_instance": None,
            "actor_id": USER,
            "slug": "acme",
            "origin": "http://localhost:3000",
        },
    },
    # -- webhook_task.model_activity (update)
    {
        "case": "model_activity_update",
        "call_site": "plane/app/views/project/base.py:370",
        "task": "plane.bgtasks.webhook_task.model_activity",
        "kwargs": {
            "model_name": "project",
            "model_id": str(PROJECT),
            "requested_data": {"name": "Apollo 2", "identifier": "APO"},
            "current_instance": jd({"name": "Apollo", "identifier": "APO"}),
            "actor_id": USER,
            "slug": "acme",
            "origin": "http://localhost:3000",
        },
    },
    # -- webhook_task.send_webhook_deactivation_email (UUID receiver)
    {
        "case": "send_webhook_deactivation_email_uuid",
        "call_site": "plane/bgtasks/webhook_task.py:359",
        "task": "plane.bgtasks.webhook_task.send_webhook_deactivation_email",
        "kwargs": {
            "webhook_id": WEBHOOK,
            "receiver_id": USER,
            "reason": "403 Client Error",
            "current_site": "http://localhost:3000",
        },
    },
    # -- webhook_task.send_webhook_deactivation_email (NULL receiver)
    {
        "case": "send_webhook_deactivation_email_none",
        "call_site": "plane/bgtasks/webhook_task.py:359",
        "task": "plane.bgtasks.webhook_task.send_webhook_deactivation_email",
        "kwargs": {
            "webhook_id": WEBHOOK,
            "receiver_id": None,
            "reason": "connection refused",
            "current_site": "http://localhost:3000",
        },
    },
    # -- webhook_task.webhook_activity (project deleted: UUID actor/event)
    {
        "case": "webhook_activity_project_deleted",
        "call_site": "plane/app/views/project/base.py:401",
        "task": "plane.bgtasks.webhook_task.webhook_activity",
        "kwargs": {
            "event": "project",
            "verb": "deleted",
            "field": None,
            "old_value": None,
            "new_value": None,
            "actor_id": USER,
            "slug": "acme",
            "current_site": "http://localhost:3000",
            "event_id": PROJECT,
            "old_identifier": None,
            "new_identifier": None,
        },
    },
    # -- webhook_task.webhook_activity (model update: string event id)
    {
        "case": "webhook_activity_updated",
        "call_site": "plane/bgtasks/webhook_task.py:507",
        "task": "plane.bgtasks.webhook_task.webhook_activity",
        "kwargs": {
            "event": "issue",
            "verb": "updated",
            "field": "priority",
            "old_value": "medium",
            "new_value": "high",
            "actor_id": USER,
            "slug": "acme",
            "current_site": "http://localhost:3000",
            "event_id": str(ISSUE),
            "old_identifier": None,
            "new_identifier": None,
        },
    },
    # -- webhook_task.webhook_send_task
    {
        "case": "webhook_send_task",
        "call_site": "plane/bgtasks/webhook_task.py:451",
        "task": "plane.bgtasks.webhook_task.webhook_send_task",
        "kwargs": {
            "webhook_id": WEBHOOK,
            "slug": "acme",
            "event": "issue",
            "event_data": {
                "id": str(ISSUE),
                "name": "Fix the login redirect loop",
                "priority": "high",
            },
            "action": "updated",
            "current_site": "http://localhost:3000",
            "activity": {
                "field": "priority",
                "new_value": "high",
                "old_value": "medium",
                "actor": {"id": str(USER), "email": "ada@example.com"},
                "old_identifier": None,
                "new_identifier": None,
            },
        },
    },
    # -- work_item_link_task.crawl_work_item_link_title (string id)
    {
        "case": "crawl_work_item_link_title_string",
        "call_site": "plane/app/views/issue/link.py:61",
        "task": "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
        "args": [str(LINK), "https://example.com/docs"],
    },
    # -- work_item_link_task.crawl_work_item_link_title (UUID id)
    {
        "case": "crawl_work_item_link_title_uuid",
        "call_site": "plane/api/views/issue.py:1284",
        "task": "plane.bgtasks.work_item_link_task.crawl_work_item_link_title",
        "args": [LINK, "https://example.com/docs"],
    },
    # -- workspace_invitation_task.workspace_invitation
    {
        "case": "workspace_invitation",
        "call_site": "plane/app/views/workspace/invite.py:120",
        "task": "plane.bgtasks.workspace_invitation_task.workspace_invitation",
        "args": [
            "invitee@example.com",
            WORKSPACE,
            "eyJhbGciOiJIUzI1NiJ9.invitation-token.signature",
            "http://localhost:3000",
            "admin@example.com",
        ],
    },
    # -- workspace_seed_task.workspace_seed
    {
        "case": "workspace_seed",
        "call_site": "plane/app/views/workspace/base.py:138",
        "task": "plane.bgtasks.workspace_seed_task.workspace_seed",
        "args": [str(WORKSPACE)],
    },
    # -- Beat-only, no-argument tasks
    {
        "case": "delete_api_logs",
        "call_site": "plane/celery.py:78 (beat_schedule)",
        "task": "plane.bgtasks.cleanup_task.delete_api_logs",
    },
    {
        "case": "delete_email_notification_logs",
        "call_site": "plane/celery.py:81 (beat_schedule)",
        "task": "plane.bgtasks.cleanup_task.delete_email_notification_logs",
    },
    {
        "case": "delete_issue_description_versions",
        "call_site": "plane/celery.py:89 (beat_schedule)",
        "task": "plane.bgtasks.cleanup_task.delete_issue_description_versions",
    },
    {
        "case": "delete_page_versions",
        "call_site": "plane/celery.py:85 (beat_schedule)",
        "task": "plane.bgtasks.cleanup_task.delete_page_versions",
    },
    {
        "case": "delete_webhook_logs",
        "call_site": "plane/celery.py:93 (beat_schedule)",
        "task": "plane.bgtasks.cleanup_task.delete_webhook_logs",
    },
    {
        "case": "hard_delete",
        "call_site": "plane/celery.py:61 (beat_schedule)",
        "task": "plane.bgtasks.deletion_task.hard_delete",
    },
    {
        "case": "delete_old_s3_link",
        "call_site": "plane/celery.py:69 (beat_schedule)",
        "task": "plane.bgtasks.exporter_expired_task.delete_old_s3_link",
    },
    {
        "case": "delete_unuploaded_file_asset",
        "call_site": "plane/celery.py:73 (beat_schedule)",
        "task": "plane.bgtasks.file_asset_task.delete_unuploaded_file_asset",
    },
    {
        "case": "archive_and_close_old_issues",
        "call_site": "plane/celery.py:65 (beat_schedule)",
        "task": "plane.bgtasks.issue_automation_task.archive_and_close_old_issues",
    },
    {
        "case": "unsnooze_expired_intake_issues",
        "call_site": "plane/celery.py:52 (beat_schedule)",
        "task": "plane.bgtasks.issue_automation_task.unsnooze_expired_intake_issues",
    },
    {
        "case": "stack_email_notification",
        "call_site": "plane/celery.py:48 (beat_schedule)",
        "task": "plane.bgtasks.email_notification_task.stack_email_notification",
    },
    {
        "case": "push_instance_metrics",
        "call_site": "plane/license/management/commands/register_instance.py:90",
        "task": "plane.license.bgtasks.telemetry_metrics.push_instance_metrics",
    },
    # -- Raw request-body values the export constructors pass through unchanged
    #    (plans/ANALYTICS_EXPORTS.md §2b): `project_ids=request.data.get(
    #    "project", [])` and `multiple=request.data.get("multiple", False)`
    #    (plane/app/views/exporter/base.py:29-30), `data=request.data`
    #    (plane/app/views/analytic/base.py:256). Appended at the end so every
    #    earlier case keeps its position-derived task id.
    {
        "case": "issue_export_task_raw_string",
        "call_site": "plane/app/views/exporter/base.py:50 (project/multiple as sent)",
        "task": "plane.bgtasks.export_task.issue_export_task",
        "kwargs": {
            "provider": "csv",
            "workspace_id": WORKSPACE,
            "project_ids": "single-project-id",
            "token_id": "exporter-token",
            "multiple": "false",
            "slug": "acme",
        },
    },
    {
        "case": "issue_export_task_raw_ints",
        "call_site": "plane/app/views/exporter/base.py:50 (project/multiple as sent)",
        "task": "plane.bgtasks.export_task.issue_export_task",
        "kwargs": {
            "provider": "xlsx",
            "workspace_id": WORKSPACE,
            "project_ids": [1, 2, 3],
            "token_id": "exporter-token",
            "multiple": 0,
            "slug": "acme",
        },
    },
    {
        "case": "issue_export_task_raw_nested",
        "call_site": "plane/app/views/exporter/base.py:50 (project/multiple as sent)",
        "task": "plane.bgtasks.export_task.issue_export_task",
        "kwargs": {
            "provider": "csv",
            "workspace_id": WORKSPACE,
            "project_ids": [["nested"], {"k": [1, None, True]}],
            "token_id": "exporter-token",
            "multiple": {"form": ["false"]},
            "slug": "acme",
        },
    },
    {
        "case": "analytic_export_task_raw_list",
        "call_site": "plane/app/views/analytic/base.py:256 (data=request.data)",
        "task": "plane.bgtasks.analytic_plot_export.analytic_export_task",
        "kwargs": {
            "email": "admin@example.com",
            "data": ["x_axis", 3, None, {"nested": [True, 2.5]}],
            "slug": "acme",
            "user_id": str(USER),
        },
    },
    {
        "case": "analytic_export_task_raw_int",
        "call_site": "plane/app/views/analytic/base.py:256 (data=request.data)",
        "task": "plane.bgtasks.analytic_plot_export.analytic_export_task",
        "kwargs": {
            "email": "admin@example.com",
            "data": 7,
            "slug": "acme",
            "user_id": str(USER),
        },
    },
]


def numbered() -> list[dict[str, Any]]:
    """Return the cases with their deterministic ids attached."""
    return [{**case, "id": case_id(index + 1)} for index, case in enumerate(CASES)]


if __name__ == "__main__":
    print(f"{len(CASES)} cases")
    for case in numbered():
        print(f"  {case['id']}  {case['case']:<48} {case['task']}")
