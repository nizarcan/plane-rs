// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Representative task messages shared by the golden test, the live check and
//! `tools/celery/`.
//!
//! Each case mirrors one Python call site from `apps/server/TASKS.md` §5 and
//! is recorded by `tools/celery/record.py`. The case ids here must match the
//! `case` fields in `tools/celery/golden/messages.json`; the golden test
//! asserts that they do.
//!
//! This module is compiled for unit tests and behind the `test-cases` feature;
//! it is not part of a production build.

use uuid::Uuid;

use crate::celery::args::TaskArg;
use crate::celery::message::Message;
use crate::celery::tasks::{
    BackfillBatch, IssueActivityBase, IssueActivityCall, ModelActivityCall, NotificationsCall,
    RecentVisitedTask, WebhookActivityCall, WebhookSendTaskCall, analytic_export_task,
    archive_and_close_old_issues, copy_s3_objects_of_description_and_assets,
    crawl_work_item_link_title, delete_api_logs, delete_email_notification_logs,
    delete_issue_description_versions, delete_old_s3_link, delete_page_versions,
    delete_unuploaded_file_asset, delete_webhook_logs, forgot_password, get_asset_object_metadata,
    get_asset_object_metadata_positional, hard_delete, issue_activity,
    issue_description_version_task, issue_export_task, magic_link, model_activity, notifications,
    page_transaction, process_logs, project_add_user_email, push_instance_metrics,
    recent_visited_task, schedule_issue_description_version, schedule_issue_version,
    send_email_notification, send_email_update_confirmation, send_email_update_magic_code,
    send_webhook_deactivation_email, soft_delete_related_objects, stack_email_notification,
    sync_issue_description_version, sync_issue_version, track_page_version,
    unsnooze_expired_intake_issues, user_activation_email, user_deactivation_email,
    webhook_activity, webhook_send_task, workspace_invitation, workspace_seed,
};

const WORKSPACE: Uuid = Uuid::from_u128(0x6a1b6f0e_0f9a_4b62_9a39_1b2c3d4e5f60);
const USER: Uuid = Uuid::from_u128(0x0d1e2f30_4456_4789_9abc_def012345678);
const ISSUE: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);
const PROJECT: Uuid = Uuid::from_u128(0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);
const PAGE: Uuid = Uuid::from_u128(0x44444444_5555_6666_7777_888888888888);
const LINK: Uuid = Uuid::from_u128(0x66666666_7777_8888_9999_aaaaaaaaaaaa);
const WEBHOOK: Uuid = Uuid::from_u128(0x77777777_8888_9999_aaaa_bbbbbbbbbbbb);
const EMAIL_ID: Uuid = Uuid::from_u128(0x88888888_9999_aaaa_bbbb_cccccccccccc);
const INVITE: Uuid = Uuid::from_u128(0x99999999_aaaa_bbbb_cccc_dddddddddddd);

const USER_TEXT: &str = "0d1e2f30-4456-4789-9abc-def012345678";
const ISSUE_TEXT: &str = "11111111-2222-3333-4444-555555555555";
const PROJECT_TEXT: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const PAGE_TEXT: &str = "44444444-5555-6666-7777-888888888888";
const LINK_TEXT: &str = "66666666-7777-8888-9999-aaaaaaaaaaaa";
const WORKSPACE_TEXT: &str = "6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60";
const ASSET_TEXT: &str = "99999999-8888-7777-6666-555555555555";
const DISPATCH_TEXT: &str = "12345678-1234-5678-1234-567812345678";

fn s(value: &str) -> TaskArg {
    TaskArg::str(value)
}

fn u(value: Uuid) -> TaskArg {
    TaskArg::uuid(value)
}

fn js(value: &str) -> TaskArg {
    TaskArg::json_text(value)
}

fn map(entries: &[(&str, TaskArg)]) -> TaskArg {
    TaskArg::Map(
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect(),
    )
}

fn list(items: &[TaskArg]) -> TaskArg {
    TaskArg::List(items.to_vec())
}

/// `request.data` for one of the export tasks: the value `pyjson` parses from
/// a JSON body, in the inline (`TaskArg::Json`) form a handler passes.
fn body_json(text: &str) -> TaskArg {
    TaskArg::json(
        plane_serde::pyjson::loads(text.as_bytes(), "utf-8")
            .unwrap_or_else(|error| panic!("test JSON {text:?} does not parse: {error}")),
    )
}

/// Build one representative message by its case id.
///
/// Returns `None` for an unknown id so a tool can report a useful error.
#[must_use]
pub fn build(case: &str) -> Option<Message> {
    Some(match case {
        "analytic_export_task" => analytic_export_task(
            "admin@example.com".to_string(),
            map(&[
                ("project_ids", list(&[s(PROJECT_TEXT)])),
                (
                    "filters",
                    map(&[
                        ("priority", list(&[s("high"), s("urgent")])),
                        ("estimate", TaskArg::Float(2.5)),
                    ]),
                ),
                ("group_by", TaskArg::Null),
                ("x_axis", s("state")),
            ]),
            "acme".to_string(),
            USER_TEXT.to_string(),
        ),
        "copy_s3_objects" => copy_s3_objects_of_description_and_assets(
            "PAGE".to_string(),
            PAGE,
            PROJECT,
            "acme".to_string(),
            USER,
        ),
        "soft_delete_related_objects_using" => soft_delete_related_objects(
            "db".to_string(),
            "issue".to_string(),
            ISSUE,
            Some("default".to_string()),
        ),
        "soft_delete_related_objects_no_using" => {
            soft_delete_related_objects("db".to_string(), "project".to_string(), PROJECT, None)
        }
        "send_email_notification" => send_email_notification(
            ISSUE,
            map(&[(
                USER_TEXT,
                list(&[map(&[
                    (
                        "issue_activity",
                        map(&[
                            ("id", s(DISPATCH_TEXT)),
                            ("verb", s("updated")),
                            ("field", s("priority")),
                            ("actor", s(USER_TEXT)),
                            ("new_value", s("high")),
                            ("old_value", s("medium")),
                            ("old_identifier", TaskArg::Null),
                            ("new_identifier", TaskArg::Null),
                            ("activity_time", s("2026-11-15 17:24:04")),
                        ]),
                    ),
                    (
                        "issue",
                        map(&[
                            ("id", s(ISSUE_TEXT)),
                            ("name", s("Fix the login redirect loop")),
                        ]),
                    ),
                ])]),
            )]),
            USER_TEXT.to_string(),
            vec![EMAIL_ID, INVITE],
        ),
        "issue_export_task" => issue_export_task(
            "csv".to_string(),
            WORKSPACE,
            list(&[s(PROJECT_TEXT), s(PROJECT_TEXT)]),
            "exporter-token".to_string(),
            TaskArg::Bool(true),
            "acme".to_string(),
        ),
        // Raw request-body values for the two export constructors
        // (`plans/ANALYTICS_EXPORTS.md` §2b): `project_ids`/`multiple` are
        // `request.data.get(...)` and `data` is `request.data`, so a handler
        // hands over whatever `pyjson` parsed — not a normalised list/bool.
        "issue_export_task_raw_string" => issue_export_task(
            "csv".to_string(),
            WORKSPACE,
            body_json(r#""single-project-id""#),
            "exporter-token".to_string(),
            body_json(r#""false""#),
            "acme".to_string(),
        ),
        "issue_export_task_raw_ints" => issue_export_task(
            "xlsx".to_string(),
            WORKSPACE,
            body_json("[1, 2, 3]"),
            "exporter-token".to_string(),
            body_json("0"),
            "acme".to_string(),
        ),
        "issue_export_task_raw_nested" => issue_export_task(
            "csv".to_string(),
            WORKSPACE,
            body_json(r#"[["nested"], {"k": [1, null, true]}]"#),
            "exporter-token".to_string(),
            body_json(r#"{"form": ["false"]}"#),
            "acme".to_string(),
        ),
        "analytic_export_task_raw_list" => analytic_export_task(
            "admin@example.com".to_string(),
            body_json(r#"["x_axis", 3, null, {"nested": [true, 2.5]}]"#),
            "acme".to_string(),
            USER_TEXT.to_string(),
        ),
        "analytic_export_task_raw_int" => analytic_export_task(
            "admin@example.com".to_string(),
            body_json("7"),
            "acme".to_string(),
            USER_TEXT.to_string(),
        ),
        "forgot_password" => forgot_password(
            "Ada".to_string(),
            "ada@example.com".to_string(),
            "dXNlcjox".to_string(),
            "abc-def".to_string(),
            "http://localhost:3000".to_string(),
        ),
        "issue_activity_view" => issue_activity(IssueActivityCall::view(
            IssueActivityBase {
                activity_type: "issue.activity.updated".to_string(),
                requested_data: js(
                    r#"{"name": "Fix the login redirect loop", "priority": "high", "assignees": ["0d1e2f30-4456-4789-9abc-def012345678"], "target_date": "2026-11-20"}"#,
                ),
                actor_id: USER_TEXT.to_string(),
                issue_id: s(ISSUE_TEXT),
                project_id: s(PROJECT_TEXT),
                current_instance: js(
                    r#"{"name": "Fix login redirect", "priority": "medium", "assignees": [], "target_date": null}"#,
                ),
                epoch: 1794764767,
            },
            "http://localhost:3000",
        )),
        "issue_activity_bare" => issue_activity(IssueActivityCall::bare(IssueActivityBase {
            activity_type: "issue.activity.deleted".to_string(),
            requested_data: js(r#"{"issue_id": "11111111-2222-3333-4444-555555555555"}"#),
            actor_id: USER_TEXT.to_string(),
            issue_id: s(ISSUE_TEXT),
            project_id: s(PROJECT_TEXT),
            current_instance: js(r#"{"name": "Fix login redirect"}"#),
            epoch: 1794764767,
        })),
        "issue_activity_view_with_intake_notification" => {
            issue_activity(IssueActivityCall::view_with_intake(
                IssueActivityBase {
                    activity_type: "issue.activity.created".to_string(),
                    requested_data: js(r#"{"name": "Inbox item"}"#),
                    actor_id: USER_TEXT.to_string(),
                    issue_id: s(ISSUE_TEXT),
                    project_id: s(PROJECT_TEXT),
                    current_instance: TaskArg::Null,
                    epoch: 1794764767,
                },
                "http://localhost:3000",
                true,
                INVITE.hyphenated().to_string(),
            ))
        }
        "issue_activity_view_with_intake_no_notification" => {
            issue_activity(IssueActivityCall::view_with_intake(
                IssueActivityBase {
                    activity_type: "intake.activity.created".to_string(),
                    requested_data: js(r#"{"status": -1}"#),
                    actor_id: USER_TEXT.to_string(),
                    issue_id: s(ISSUE_TEXT),
                    project_id: s(PROJECT_TEXT),
                    current_instance: js(r#"{"status": -2}"#),
                    epoch: 1794764767,
                },
                "http://localhost:3000",
                false,
                INVITE.hyphenated().to_string(),
            ))
        }
        "issue_activity_automation" => {
            issue_activity(IssueActivityCall::automation(IssueActivityBase {
                activity_type: "issue.activity.updated".to_string(),
                requested_data: js(r#"{"archived_at": "2026-10-01", "automation": true}"#),
                actor_id: USER_TEXT.to_string(),
                issue_id: u(ISSUE),
                project_id: u(PROJECT),
                current_instance: js(r#"{"archived_at": null}"#),
                epoch: 1794764767,
            }))
        }
        "issue_activity_bulk_date" => {
            issue_activity(IssueActivityCall::bulk_date(IssueActivityBase {
                activity_type: "issue.activity.updated".to_string(),
                requested_data: js(r#"{"start_date": "2026-11-20"}"#),
                actor_id: USER_TEXT.to_string(),
                issue_id: s(ISSUE_TEXT),
                project_id: s(PROJECT_TEXT),
                current_instance: js(r#"{"start_date": "2026-11-01"}"#),
                epoch: 1794764767,
            }))
        }
        "issue_activity_with_intake" => issue_activity(IssueActivityCall::with_intake(
            IssueActivityBase {
                activity_type: "issue.activity.created".to_string(),
                requested_data: js(r#"{"name": "Inbox item"}"#),
                actor_id: USER_TEXT.to_string(),
                issue_id: s(ISSUE_TEXT),
                project_id: s(PROJECT_TEXT),
                current_instance: TaskArg::Null,
                epoch: 1794764767,
            },
            INVITE.hyphenated().to_string(),
        )),
        "issue_activity_with_origin" => issue_activity(IssueActivityCall::with_origin(
            IssueActivityBase {
                activity_type: "module.activity.deleted".to_string(),
                requested_data: js(r#"{"module_id": "22222222-3333-4444-5555-666666666666"}"#),
                actor_id: USER_TEXT.to_string(),
                issue_id: TaskArg::Null,
                project_id: s(PROJECT_TEXT),
                current_instance: js(r#"{"module_name": "Roadmap"}"#),
                epoch: 1794764767,
            },
            "http://localhost:3000",
        )),
        "issue_activity_view_with_subscriber" => {
            issue_activity(IssueActivityCall::view_with_subscriber(
                IssueActivityBase {
                    activity_type: "issue.activity.deleted".to_string(),
                    requested_data: js(r#"{"issue_id": "11111111-2222-3333-4444-555555555555"}"#),
                    actor_id: USER_TEXT.to_string(),
                    issue_id: s(ISSUE_TEXT),
                    project_id: s(PROJECT_TEXT),
                    current_instance: map(&[]),
                    epoch: 1794764767,
                },
                "http://localhost:3000",
            ))
        }
        "issue_activity_link_created" => {
            issue_activity(IssueActivityCall::link_created(IssueActivityBase {
                activity_type: "link.activity.created".to_string(),
                requested_data: js(
                    r#"{"id": "66666666-7777-8888-9999-aaaaaaaaaaaa", "title": "Example", "url": "https://example.com"}"#,
                ),
                actor_id: USER_TEXT.to_string(),
                issue_id: s(ISSUE_TEXT),
                project_id: s(PROJECT_TEXT),
                current_instance: TaskArg::Null,
                epoch: 1794764767,
            }))
        }
        "schedule_issue_description_version" => schedule_issue_description_version(s("5000"), 300),
        "sync_issue_description_version_continue" => {
            sync_issue_description_version(BackfillBatch::Continue {
                batch_size: 5000,
                offset: 5000,
                countdown: 300,
            })
        }
        "sync_issue_description_version_start" => {
            sync_issue_description_version(BackfillBatch::Start {
                batch_size: TaskArg::Int(5000),
                countdown: 300,
            })
        }
        "issue_description_version_task_create" => issue_description_version_task(
            js(r#"{"name": "New issue", "description_html": "<p>hi</p>"}"#),
            ISSUE_TEXT.to_string(),
            USER,
            true,
        ),
        "issue_description_version_task_update" => issue_description_version_task(
            js(r#"{"name": "Updated issue", "description_html": "<p>bye</p>"}"#),
            ISSUE_TEXT.to_string(),
            USER,
            false,
        ),
        "schedule_issue_version" => schedule_issue_version(s("5000"), 300),
        "sync_issue_version_continue" => sync_issue_version(BackfillBatch::Continue {
            batch_size: 5000,
            offset: 5000,
            countdown: 300,
        }),
        "sync_issue_version_start" => sync_issue_version(BackfillBatch::Start {
            batch_size: TaskArg::Int(5000),
            countdown: 300,
        }),
        "process_logs" => process_logs(map(&[
            ("token_identifier", s("b3f1c0d2e3a4")),
            ("path", s("/api/v1/workspaces/acme/projects/")),
            ("method", s("GET")),
            ("query_params", s("per_page=20")),
            ("headers", s("{'User-Agent': 'curl/8.7.1'}")),
            ("body", TaskArg::Null),
            ("response_body", s(r#"{"count": 1}"#)),
            ("response_code", TaskArg::Int(200)),
            ("ip_address", s("203.0.113.7")),
            ("user_agent", s("curl/8.7.1")),
        ])),
        "magic_link" => magic_link(
            "ada@example.com".to_string(),
            "magic-key".to_string(),
            "magic-token".to_string(),
        ),
        "notifications_uuid_ids" => notifications(NotificationsCall {
            activity_type: "issue.activity.updated".to_string(),
            issue_id: u(ISSUE),
            actor_id: s(USER_TEXT),
            project_id: u(PROJECT),
            subscriber: true,
            issue_activities_created: js(
                r#"[{"id": "12345678-1234-5678-1234-567812345678", "verb": "updated", "field": "priority", "actor": "0d1e2f30-4456-4789-9abc-def012345678"}]"#,
            ),
            requested_data: js(r#"{"priority": "high"}"#),
            current_instance: js(r#"{"priority": "medium"}"#),
        }),
        "notifications_string_ids" => notifications(NotificationsCall {
            activity_type: "cycle.activity.created".to_string(),
            issue_id: TaskArg::Null,
            actor_id: s(USER_TEXT),
            project_id: s(PROJECT_TEXT),
            subscriber: false,
            issue_activities_created: js("[]"),
            requested_data: TaskArg::Null,
            current_instance: js(r#"{"created_cycle_issues": []}"#),
        }),
        "page_transaction_create" => page_transaction(
            "<p>Hello 'world' — café 😀 \\ path</p>".to_string(),
            None,
            s(PAGE_TEXT),
        ),
        "page_transaction_update" => page_transaction(
            "<p>Hello \"world\"</p>\n<p>Second line</p>".to_string(),
            Some("<p>Hello</p>".to_string()),
            u(PAGE),
        ),
        "track_page_version" => track_page_version(
            PAGE,
            js(
                r#"{"id": "44444444-5555-6666-7777-888888888888", "name": "Doc", "description_html": "<p>Hi</p>"}"#,
            ),
            USER,
        ),
        "project_add_user_email" => {
            project_add_user_email("http://localhost:3000".to_string(), INVITE, USER)
        }
        "recent_visited_project" => recent_visited_task(RecentVisitedTask::ProjectKeyed {
            slug: "acme".to_string(),
            project_id: Some(u(PROJECT)),
            entity_name: "project".to_string(),
            entity_identifier: u(PROJECT),
            user_id: u(USER),
        }),
        "recent_visited_entity" => recent_visited_task(RecentVisitedTask::EntityKeyed {
            slug: "acme".to_string(),
            entity_name: "view".to_string(),
            entity_identifier: s(PAGE_TEXT),
            user_id: s(USER_TEXT),
            project_id: None,
        }),
        "get_asset_object_metadata_kwarg" => get_asset_object_metadata(ASSET_TEXT.to_string()),
        "get_asset_object_metadata_positional" => {
            get_asset_object_metadata_positional(ASSET_TEXT.to_string())
        }
        "user_activation_email" => user_activation_email("http://localhost:3000".to_string(), USER),
        "user_deactivation_email" => {
            user_deactivation_email("http://localhost:3000".to_string(), USER)
        }
        "send_email_update_confirmation" => {
            send_email_update_confirmation("new@example.com".to_string())
        }
        "send_email_update_magic_code" => {
            send_email_update_magic_code("new@example.com".to_string(), "123456".to_string())
        }
        "model_activity_create" => model_activity(ModelActivityCall {
            model_name: "project".to_string(),
            model_id: PROJECT_TEXT.to_string(),
            requested_data: map(&[("name", s("Apollo")), ("identifier", s("APO"))]),
            current_instance: None,
            actor_id: USER,
            slug: "acme".to_string(),
            origin: "http://localhost:3000".to_string(),
        }),
        "model_activity_update" => model_activity(ModelActivityCall {
            model_name: "project".to_string(),
            model_id: PROJECT_TEXT.to_string(),
            requested_data: map(&[("name", s("Apollo 2")), ("identifier", s("APO"))]),
            current_instance: Some(r#"{"name": "Apollo", "identifier": "APO"}"#.to_string()),
            actor_id: USER,
            slug: "acme".to_string(),
            origin: "http://localhost:3000".to_string(),
        }),
        "send_webhook_deactivation_email_uuid" => send_webhook_deactivation_email(
            WEBHOOK,
            Some(USER),
            "403 Client Error".to_string(),
            "http://localhost:3000".to_string(),
        ),
        "send_webhook_deactivation_email_none" => send_webhook_deactivation_email(
            WEBHOOK,
            None,
            "connection refused".to_string(),
            "http://localhost:3000".to_string(),
        ),
        "webhook_activity_project_deleted" => webhook_activity(WebhookActivityCall {
            event: "project".to_string(),
            verb: "deleted".to_string(),
            field: None,
            old_value: TaskArg::Null,
            new_value: TaskArg::Null,
            actor_id: USER,
            slug: "acme".to_string(),
            current_site: "http://localhost:3000".to_string(),
            event_id: u(PROJECT),
            old_identifier: None,
            new_identifier: None,
        }),
        "webhook_activity_updated" => webhook_activity(WebhookActivityCall {
            event: "issue".to_string(),
            verb: "updated".to_string(),
            field: Some("priority".to_string()),
            old_value: s("medium"),
            new_value: s("high"),
            actor_id: USER,
            slug: "acme".to_string(),
            current_site: "http://localhost:3000".to_string(),
            event_id: s(ISSUE_TEXT),
            old_identifier: None,
            new_identifier: None,
        }),
        "webhook_send_task" => webhook_send_task(WebhookSendTaskCall {
            webhook_id: WEBHOOK,
            slug: "acme".to_string(),
            event: "issue".to_string(),
            event_data: map(&[
                ("id", s(ISSUE_TEXT)),
                ("name", s("Fix the login redirect loop")),
                ("priority", s("high")),
            ]),
            action: "updated".to_string(),
            current_site: "http://localhost:3000".to_string(),
            activity: map(&[
                ("field", s("priority")),
                ("new_value", s("high")),
                ("old_value", s("medium")),
                (
                    "actor",
                    map(&[("id", s(USER_TEXT)), ("email", s("ada@example.com"))]),
                ),
                ("old_identifier", TaskArg::Null),
                ("new_identifier", TaskArg::Null),
            ]),
        }),
        "crawl_work_item_link_title_string" => {
            crawl_work_item_link_title(s(LINK_TEXT), "https://example.com/docs".to_string())
        }
        "crawl_work_item_link_title_uuid" => {
            crawl_work_item_link_title(u(LINK), "https://example.com/docs".to_string())
        }
        "workspace_invitation" => workspace_invitation(
            "invitee@example.com".to_string(),
            WORKSPACE,
            "eyJhbGciOiJIUzI1NiJ9.invitation-token.signature".to_string(),
            "http://localhost:3000".to_string(),
            "admin@example.com".to_string(),
        ),
        "workspace_seed" => workspace_seed(WORKSPACE_TEXT.to_string()),
        "delete_api_logs" => delete_api_logs(),
        "delete_email_notification_logs" => delete_email_notification_logs(),
        "delete_issue_description_versions" => delete_issue_description_versions(),
        "delete_page_versions" => delete_page_versions(),
        "delete_webhook_logs" => delete_webhook_logs(),
        "hard_delete" => hard_delete(),
        "delete_old_s3_link" => delete_old_s3_link(),
        "delete_unuploaded_file_asset" => delete_unuploaded_file_asset(),
        "archive_and_close_old_issues" => archive_and_close_old_issues(),
        "unsnooze_expired_intake_issues" => unsnooze_expired_intake_issues(),
        "stack_email_notification" => stack_email_notification(),
        "push_instance_metrics" => push_instance_metrics(),
        _ => return None,
    })
}

/// Every case id this module knows.
#[must_use]
pub fn case_ids() -> &'static [&'static str] {
    &[
        "analytic_export_task",
        "copy_s3_objects",
        "soft_delete_related_objects_using",
        "soft_delete_related_objects_no_using",
        "send_email_notification",
        "issue_export_task",
        "forgot_password",
        "issue_activity_view",
        "issue_activity_bare",
        "issue_activity_view_with_intake_notification",
        "issue_activity_view_with_intake_no_notification",
        "issue_activity_automation",
        "issue_activity_bulk_date",
        "issue_activity_with_intake",
        "issue_activity_with_origin",
        "issue_activity_view_with_subscriber",
        "issue_activity_link_created",
        "schedule_issue_description_version",
        "sync_issue_description_version_continue",
        "sync_issue_description_version_start",
        "issue_description_version_task_create",
        "issue_description_version_task_update",
        "schedule_issue_version",
        "sync_issue_version_continue",
        "sync_issue_version_start",
        "process_logs",
        "magic_link",
        "notifications_uuid_ids",
        "notifications_string_ids",
        "page_transaction_create",
        "page_transaction_update",
        "track_page_version",
        "project_add_user_email",
        "recent_visited_project",
        "recent_visited_entity",
        "get_asset_object_metadata_kwarg",
        "get_asset_object_metadata_positional",
        "user_activation_email",
        "user_deactivation_email",
        "send_email_update_confirmation",
        "send_email_update_magic_code",
        "model_activity_create",
        "model_activity_update",
        "send_webhook_deactivation_email_uuid",
        "send_webhook_deactivation_email_none",
        "webhook_activity_project_deleted",
        "webhook_activity_updated",
        "webhook_send_task",
        "crawl_work_item_link_title_string",
        "crawl_work_item_link_title_uuid",
        "workspace_invitation",
        "workspace_seed",
        "delete_api_logs",
        "delete_email_notification_logs",
        "delete_issue_description_versions",
        "delete_page_versions",
        "delete_webhook_logs",
        "hard_delete",
        "delete_old_s3_link",
        "delete_unuploaded_file_asset",
        "archive_and_close_old_issues",
        "unsnooze_expired_intake_issues",
        "stack_email_notification",
        "push_instance_metrics",
        "issue_export_task_raw_string",
        "issue_export_task_raw_ints",
        "issue_export_task_raw_nested",
        "analytic_export_task_raw_list",
        "analytic_export_task_raw_int",
    ]
}
