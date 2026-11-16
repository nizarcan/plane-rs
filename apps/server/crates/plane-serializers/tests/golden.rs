// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-equality tests against DRF's own output.
//!
//! The corpus is `tests/golden/serializers.jsonl`, recorded by
//! `apps/server/tools/golden/serializers_record.py` from a real Django process
//! against a cloned dev database (`tools/golden/serializers_fixture.py`). Each
//! line is one render: a serializer class, a subject, `fields=`/`expand=` (as
//! the view passes them), the active timezone, and
//! `JSONRenderer().render(serializer.data)` as a string.
//!
//! This file builds the *same input values* by hand (see [`Fixture`]) and
//! renders through the generated structs, asserting byte equality. The values
//! mirror `tools/golden/serializers_fixture.py`; keep the two in step. The
//! 2026-11-01 timestamps are the US DST fall-back boundary, so the
//! `America/New_York` cases render both `-04:00` and `-05:00` offsets.
//!
//! What this covers beyond "the fields are in order":
//!
//! * `DynamicBaseSerializer` with `fields=` (including the dict-item form), with
//!   `expand=` (injected at the end, replacing a base field in place, `null`
//!   relations, `issue_attachments` last) and with both together;
//! * `plane.api.serializers.base.BaseSerializer`, which removes fields for
//!   `fields=` and only ever replaces on `expand=`;
//! * `SkipField`: `StateSerializer.order` and the issue annotations that the
//!   child issue in `issue/many` does not carry (`cycle_id`, `module_ids`, …);
//! * JSON columns (`PgJson` → `transcode_json`: Postgres key order, Python
//!   floats, `ensure_ascii=False`, U+2028 escaped);
//! * the field classes Plane's models barely use (probe: decimal with and
//!   without `coerce_to_string`, time with microseconds, binary as base64).

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use plane_serializers::generated::api_issue::IssueLiteSerializer as V1IssueLite;
use plane_serializers::generated::api_workspace::WorkspaceLiteSerializer as V1WorkspaceLite;
use plane_serializers::generated::app_cycle::CycleSerializer as AppCycle;
use plane_serializers::generated::app_issue::{
    IssueAttachmentLiteSerializer, IssueDetailSerializer, IssueFlatSerializer, IssueSerializer,
    IssueStateSerializer, LabelLiteSerializer, LabelSerializer,
};
use plane_serializers::generated::app_module::ModuleSerializer;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_state::{StateLiteSerializer, StateSerializer};
use plane_serializers::generated::app_user::{UserLiteSerializer, UserMeSerializer};
use plane_serializers::generated::app_workspace::{
    WorkSpaceMemberSerializer, WorkSpaceSerializer, WorkspaceLiteSerializer,
};
use plane_serializers::{
    DrfDecimal, DrfError, DrfPk, DrfSerialize, DrfUuid, DrfValue, EncoderDecimal, Expansion,
    Expansions, FieldEntry, FilterMode, PgJson, PyBase64, PyDate, PyDecimal, PyTime, RenderCtx,
};
use serde::Deserialize;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Golden record
// ---------------------------------------------------------------------------

const CORPUS: &str = include_str!("golden/serializers.jsonl");

#[derive(Debug, Deserialize)]
struct Record {
    case: String,
    serializer: String,
    subject: String,
    #[serde(default)]
    many: bool,
    tz: String,
    #[serde(default)]
    fields: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    expand: Option<Vec<String>>,
    out: String,
}

fn records() -> Vec<Record> {
    CORPUS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("golden record"))
        .collect()
}

// ---------------------------------------------------------------------------
// Ids and timestamps (mirror serializers_fixture.py)
// ---------------------------------------------------------------------------

const USER_ALICE: Uuid = uuid::uuid!("11111111-1111-4111-8111-111111111111");
const USER_BOB: Uuid = uuid::uuid!("22222222-2222-4222-8222-222222222222");
const USER_BOT: Uuid = uuid::uuid!("33333333-3333-4333-8333-333333333333");
const USER_EMPTY: Uuid = uuid::uuid!("44444444-4444-4444-8444-444444444444");
const WORKSPACE_ID: Uuid = uuid::uuid!("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa");
const PROJECT_ID: Uuid = uuid::uuid!("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
const STATE_ID: Uuid = uuid::uuid!("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
const STATE_TRIAGE_ID: Uuid = uuid::uuid!("c1c1c1c1-c1c1-41c1-81c1-c1c1c1c1c1c1");
const LABEL_ID: Uuid = uuid::uuid!("dddddddd-dddd-4ddd-8ddd-dddddddddddd");
const ISSUE_ID: Uuid = uuid::uuid!("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee");
const ISSUE_CHILD_ID: Uuid = uuid::uuid!("e1e1e1e1-e1e1-41e1-81e1-e1e1e1e1e1e1");
const CYCLE_ID: Uuid = uuid::uuid!("ffffffff-ffff-4fff-8fff-ffffffffffff");
const MODULE_ID: Uuid = uuid::uuid!("a1b2c3d4-a1b2-41c3-81d4-a1b2c3d4a1b2");
const MEMBER_ROW_ID: Uuid = uuid::uuid!("b2c3d4e5-b2c3-42d4-83e5-b2c3d4e5b2c3");
const ATTACHMENT_ID: Uuid = uuid::uuid!("c3d4e5f6-c3d4-43e5-84f6-c3d4e5f6c3d4");
const PROBE_ID: Uuid = uuid::uuid!("99999999-9999-4999-8999-999999999999");

fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
        .single()
        .expect("valid datetime")
}

/// 2026-11-01T05:30:00Z: 01:30 EDT (-04:00) in New York.
fn t_created() -> DateTime<Utc> {
    utc(2026, 11, 1, 5, 30, 0)
}

/// 2026-11-01T06:30:00Z: 01:30 EST (-05:00) in New York, after the fall-back.
fn t_updated() -> DateTime<Utc> {
    utc(2026, 11, 1, 6, 30, 0)
}

const DESCRIPTION_JSON: &str = "{\"type\": \"doc\", \"content\": [{\"type\": \"paragraph\", \"content\": [{\"text\": \"Gear \u{1f6e0}\u{fe0f} \u{2028} line\", \"type\": \"text\"}]}]}";
const LOGO_PROPS: &str = "{\"color\": \"#abcdef\", \"emoji\": \"\u{1f3a8}\"}";
const VIEW_PROPS: &str = "{\"count\": 3, \"filters\": {\"priority\": \"high\"}}";
const PROGRESS_SNAPSHOT: &str = "{\"total\": 2, \"completed\": 1}";
const MODULE_JSON: &str = "{\"a\": 1, \"nested\": {\"b\": null, \"z\": true}}";
const ATTRIBUTES_JSON: &str = "{\"name\": \"gear.png\", \"size\": 1234}";
const MEMBER_VIEW_PROPS: &str = "{\"count\": 2, \"filters\": {\"priority\": \"high\"}}";
const BINARY: &[u8] = b"\x00\xffit's";
const ASSET_URL: &str = "/api/assets/v2/workspaces/acme/projects/bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb/issues/eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee/attachments/c3d4e5f6-c3d4-43e5-84f6-c3d4e5f6c3d4/";
const ATTACHMENT_ASSET: &str = "uploads/issues/gear.png";

fn leak_slice<T>(values: Vec<T>) -> &'static [T] {
    Box::leak(values.into_boxed_slice())
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

struct Fixture {
    alice: UserLiteSerializer<'static>,
    bob: UserLiteSerializer<'static>,
    bot: UserLiteSerializer<'static>,
    empty: UserLiteSerializer<'static>,
    user_me: UserMeSerializer<'static>,
    workspace_lite: WorkspaceLiteSerializer<'static>,
    workspace: WorkSpaceSerializer<'static>,
    project_lite: ProjectLiteSerializer<'static>,
    state: StateSerializer<'static>,
    state_with_order: StateSerializer<'static>,
    state_triage: StateSerializer<'static>,
    state_lite: StateLiteSerializer<'static>,
    label: LabelSerializer<'static>,
    label_lite: LabelLiteSerializer<'static>,
    issue: IssueSerializer<'static>,
    issue_child: IssueSerializer<'static>,
    issue_detail: IssueDetailSerializer<'static>,
    issue_flat: IssueFlatSerializer<'static>,
    attachment: IssueAttachmentLiteSerializer<'static>,
    cycle: AppCycle<'static>,
    module: ModuleSerializer<'static>,
    v1_issue_lite: V1IssueLite,
    v1_workspace_lite: V1WorkspaceLite<'static>,
    label_ids: &'static [Uuid],
    assignee_ids: &'static [Uuid],
}

fn fixture() -> Fixture {
    let alice = UserLiteSerializer {
        id: USER_ALICE,
        first_name: "Alice",
        last_name: "Anderson",
        avatar: "https://cdn.example.com/avatars/alice.png",
        avatar_url: Some("https://cdn.example.com/avatars/alice.png"),
        is_bot: false,
        display_name: "Alice A.",
    };
    let bob = UserLiteSerializer {
        id: USER_BOB,
        first_name: "Bob",
        last_name: "",
        avatar: "",
        avatar_url: None,
        is_bot: false,
        display_name: "Bob B.",
    };
    let bot = UserLiteSerializer {
        id: USER_BOT,
        first_name: "Plane",
        last_name: "Bot",
        avatar: "",
        avatar_url: None,
        is_bot: true,
        display_name: "Plane Bot",
    };
    let empty = UserLiteSerializer {
        id: USER_EMPTY,
        first_name: "",
        last_name: "",
        avatar: "",
        avatar_url: None,
        is_bot: false,
        // `User.save()` fills an empty display_name from the email local part.
        display_name: "nobody",
    };
    let user_me = UserMeSerializer {
        id: USER_ALICE,
        avatar: "https://cdn.example.com/avatars/alice.png",
        cover_image: Some("https://cdn.example.com/covers/alice.png"),
        avatar_url: Some("https://cdn.example.com/avatars/alice.png"),
        cover_image_url: Some("https://cdn.example.com/covers/alice.png"),
        date_joined: utc(2024, 1, 2, 3, 4, 5) + chrono::TimeDelta::microseconds(678_901),
        display_name: "Alice A.",
        email: Some("alice@example.com"),
        first_name: "Alice",
        last_name: "Anderson",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        user_timezone: "Asia/Kolkata",
        username: "alice",
        is_password_autoset: false,
        last_login_medium: "password",
        last_login_time: Some(utc(2025, 12, 31, 23, 59, 59)),
    };
    let workspace_lite = WorkspaceLiteSerializer {
        name: "Acme",
        slug: "acme",
        id: WORKSPACE_ID,
        logo_url: Some("https://cdn.example.com/logos/acme.png"),
    };
    let workspace = WorkSpaceSerializer {
        id: WORKSPACE_ID,
        total_members: Some(3),
        logo_url: Some("https://cdn.example.com/logos/acme.png"),
        role: Some(20),
        created_at: t_created(),
        updated_at: t_updated(),
        deleted_at: None,
        name: "Acme",
        logo: Some("https://cdn.example.com/logos/acme.png"),
        slug: "acme",
        organization_size: Some("10"),
        timezone: "America/New_York",
        background_color: "#123456",
        created_by: Some(USER_ALICE),
        updated_by: Some(USER_ALICE),
        logo_asset: None,
        owner: USER_ALICE,
    };
    let project_lite = ProjectLiteSerializer {
        id: PROJECT_ID,
        identifier: "APL",
        name: "Apollo",
        cover_image: Some("https://cdn.example.com/covers/apollo.png"),
        cover_image_url: Some("https://cdn.example.com/covers/apollo.png"),
        logo_props: Some(PgJson(LOGO_PROPS)),
        description: "Landing on the moon",
    };
    let state_lite = StateLiteSerializer {
        id: STATE_ID,
        name: "In Progress",
        color: "#f59e0b",
        group: "started",
    };
    let state = StateSerializer {
        id: STATE_ID,
        project_id: PROJECT_ID,
        workspace_id: WORKSPACE_ID,
        name: "In Progress",
        color: "#f59e0b",
        group: "started",
        default: false,
        description: "Work in progress",
        sequence: 2.5,
        order: None,
    };
    let state_with_order = StateSerializer {
        order: Some(0.5),
        ..state.clone()
    };
    let state_triage = StateSerializer {
        id: STATE_TRIAGE_ID,
        name: "Triage",
        color: "#6b7280",
        group: "triage",
        description: "",
        sequence: 0.5,
        ..state.clone()
    };
    let label = LabelSerializer {
        parent: None,
        name: "backend",
        color: "#00ff00",
        id: LABEL_ID,
        project_id: Some(PROJECT_ID),
        workspace_id: WORKSPACE_ID,
        sort_order: 65535.0,
    };
    let label_lite = LabelLiteSerializer {
        id: LABEL_ID,
        name: "backend",
        color: "#00ff00",
    };
    let label_ids = leak_slice(vec![LABEL_ID]);
    let module_ids = leak_slice(vec![MODULE_ID]);
    let assignee_ids = leak_slice(vec![USER_ALICE]);
    let member_ids = leak_slice(vec![USER_ALICE, USER_BOB]);

    let attachment = IssueAttachmentLiteSerializer {
        id: ATTACHMENT_ID,
        asset: ATTACHMENT_ASSET,
        attributes: Some(PgJson(ATTRIBUTES_JSON)),
        created_by: Some(USER_ALICE),
        updated_at: t_updated(),
        updated_by: Some(USER_ALICE),
        asset_url: Some(ASSET_URL),
    };

    let issue = IssueSerializer {
        id: ISSUE_ID,
        name: "Fix the landing gear",
        state_id: Some(STATE_ID),
        sort_order: 65535.0,
        completed_at: None,
        estimate_point: None,
        priority: "high",
        start_date: Some(NaiveDate::from_ymd_opt(2026, 10, 30).expect("date")),
        target_date: Some(NaiveDate::from_ymd_opt(2026, 11, 5).expect("date")),
        sequence_id: 42,
        project_id: PROJECT_ID,
        parent_id: None,
        cycle_id: Some(DrfPk::Uuid(CYCLE_ID)),
        module_ids: Some(module_ids),
        label_ids: Some(label_ids),
        assignee_ids: Some(assignee_ids),
        sub_issues_count: Some(1),
        created_at: t_created(),
        updated_at: t_updated(),
        created_by: Some(USER_ALICE),
        updated_by: Some(USER_BOB),
        attachment_count: Some(1),
        link_count: Some(1),
        is_draft: false,
        archived_at: None,
    };
    // The child issue carries no view annotations, so every annotation-backed
    // field is a `SkipField`: its key is absent, while the model-backed
    // `is_draft`/`archived_at` stay.
    let issue_child = IssueSerializer {
        id: ISSUE_CHILD_ID,
        name: "Inspect the strut",
        state_id: Some(STATE_ID),
        sort_order: 65535.0,
        completed_at: None,
        estimate_point: None,
        priority: "low",
        start_date: None,
        target_date: None,
        sequence_id: 43,
        project_id: PROJECT_ID,
        parent_id: Some(ISSUE_ID),
        cycle_id: None,
        module_ids: None,
        label_ids: None,
        assignee_ids: None,
        sub_issues_count: None,
        created_at: t_created(),
        updated_at: t_updated(),
        created_by: Some(USER_BOB),
        updated_by: Some(USER_BOB),
        attachment_count: None,
        link_count: None,
        is_draft: false,
        archived_at: None,
    };
    let issue_detail = IssueDetailSerializer {
        id: ISSUE_ID,
        name: "Fix the landing gear",
        state_id: Some(STATE_ID),
        sort_order: 65535.0,
        completed_at: None,
        estimate_point: None,
        priority: "high",
        start_date: Some(NaiveDate::from_ymd_opt(2026, 10, 30).expect("date")),
        target_date: Some(NaiveDate::from_ymd_opt(2026, 11, 5).expect("date")),
        sequence_id: 42,
        project_id: PROJECT_ID,
        parent_id: None,
        cycle_id: Some(DrfPk::Uuid(CYCLE_ID)),
        module_ids: Some(module_ids),
        label_ids: Some(label_ids),
        assignee_ids: Some(assignee_ids),
        sub_issues_count: Some(1),
        created_at: t_created(),
        updated_at: t_updated(),
        created_by: Some(USER_ALICE),
        updated_by: Some(USER_BOB),
        attachment_count: Some(1),
        link_count: Some(1),
        is_draft: false,
        archived_at: None,
        description_html: "<p>Gear</p>",
        is_subscribed: Some(false),
        is_intake: Some(false),
    };
    let issue_flat = IssueFlatSerializer {
        id: ISSUE_ID,
        name: "Fix the landing gear",
        description_json: Some(PgJson(DESCRIPTION_JSON)),
        description_html: "<p>Gear</p>",
        priority: "high",
        start_date: Some(NaiveDate::from_ymd_opt(2026, 10, 30).expect("date")),
        target_date: Some(NaiveDate::from_ymd_opt(2026, 11, 5).expect("date")),
        sequence_id: 42,
        sort_order: 65535.0,
        is_draft: false,
    };
    let cycle = AppCycle {
        id: CYCLE_ID,
        workspace_id: WORKSPACE_ID,
        project_id: PROJECT_ID,
        name: "Sprint 1",
        description: "First sprint",
        start_date: Some(t_created()),
        end_date: Some(utc(2026, 11, 8, 6, 30, 0)),
        owned_by_id: USER_ALICE,
        view_props: Some(PgJson(VIEW_PROPS)),
        sort_order: 65535.0,
        external_source: None,
        external_id: None,
        progress_snapshot: Some(PgJson(PROGRESS_SNAPSHOT)),
        logo_props: Some(PgJson(LOGO_PROPS)),
        is_favorite: Some(true),
        total_issues: Some(3),
        cancelled_issues: Some(0),
        completed_issues: Some(1),
        started_issues: Some(1),
        unstarted_issues: Some(1),
        backlog_issues: Some(0),
        status: Some("current"),
    };
    let module = ModuleSerializer {
        id: MODULE_ID,
        workspace_id: WORKSPACE_ID,
        project_id: PROJECT_ID,
        name: "Checkout",
        description: "Checkout revamp",
        description_text: Some(PgJson(MODULE_JSON)),
        description_html: Some(PgJson(MODULE_JSON)),
        start_date: Some(NaiveDate::from_ymd_opt(2026, 10, 27).expect("date")),
        target_date: Some(NaiveDate::from_ymd_opt(2026, 11, 20).expect("date")),
        status: "in-progress",
        lead_id: Some(USER_ALICE),
        member_ids: Some(member_ids),
        view_props: Some(PgJson(VIEW_PROPS)),
        sort_order: 65535.0,
        external_source: None,
        external_id: None,
        logo_props: Some(PgJson(LOGO_PROPS)),
        total_estimate_points: Some(13.0),
        completed_estimate_points: Some(5.0),
        is_favorite: Some(false),
        total_issues: Some(4),
        cancelled_issues: Some(0),
        completed_issues: Some(1),
        started_issues: Some(2),
        unstarted_issues: Some(1),
        backlog_issues: Some(0),
        created_at: t_created(),
        updated_at: t_updated(),
        archived_at: None,
    };
    let v1_issue_lite = V1IssueLite {
        id: ISSUE_ID,
        sequence_id: 42,
        project_id: PROJECT_ID,
    };
    let v1_workspace_lite = V1WorkspaceLite {
        name: "Acme",
        slug: "acme",
        id: WORKSPACE_ID,
    };

    Fixture {
        alice,
        bob,
        bot,
        empty,
        user_me,
        workspace_lite,
        workspace,
        project_lite,
        state,
        state_with_order,
        state_triage,
        state_lite,
        label,
        label_lite,
        issue,
        issue_child,
        issue_detail,
        issue_flat,
        attachment,
        cycle,
        module,
        v1_issue_lite,
        v1_workspace_lite,
        label_ids,
        assignee_ids,
    }
}

/// `IssueStateSerializer` is `fields = "__all__"` and declares nested
/// `label_details`/`state_detail`/`project_detail`/`assignee_details` fields, so
/// it borrows other fixture values; build it on demand instead of storing it.
fn issue_state(fx: &Fixture) -> IssueStateSerializer<'_> {
    IssueStateSerializer {
        id: ISSUE_ID,
        label_details: std::slice::from_ref(&fx.label_lite),
        state_detail: Some(&fx.state_lite),
        project_detail: Some(&fx.project_lite),
        assignee_details: std::slice::from_ref(&fx.alice),
        sub_issues_count: Some(1),
        attachment_count: Some(1),
        link_count: Some(1),
        created_at: t_created(),
        updated_at: t_updated(),
        deleted_at: None,
        point: None,
        name: "Fix the landing gear",
        description_json: Some(PgJson(DESCRIPTION_JSON)),
        description_html: "<p>Gear</p>",
        description_stripped: Some("Gear"),
        description_binary: Some(BINARY),
        priority: "high",
        start_date: Some(NaiveDate::from_ymd_opt(2026, 10, 30).expect("date")),
        target_date: Some(NaiveDate::from_ymd_opt(2026, 11, 5).expect("date")),
        sequence_id: 42,
        sort_order: 65535.0,
        completed_at: None,
        archived_at: None,
        is_draft: false,
        external_source: None,
        external_id: None,
        created_by: Some(USER_ALICE),
        updated_by: Some(USER_BOB),
        project: PROJECT_ID,
        workspace: WORKSPACE_ID,
        parent: None,
        state: Some(STATE_ID),
        estimate_point: None,
        r#type: None,
        assignees: fx.assignee_ids,
        labels: fx.label_ids,
    }
}

fn workspace_member(fx: &Fixture) -> WorkSpaceMemberSerializer<'_> {
    WorkSpaceMemberSerializer {
        id: MEMBER_ROW_ID,
        member: Some(&fx.alice),
        created_at: t_created(),
        updated_at: t_updated(),
        deleted_at: None,
        role: 20,
        company_role: Some("Engineer"),
        view_props: Some(PgJson(MEMBER_VIEW_PROPS)),
        default_props: Some(PgJson("{\"is_favorite\": false}")),
        issue_props: Some(PgJson("{\"layout\": \"list\"}")),
        is_active: true,
        getting_started_checklist: Some(PgJson("{\"completed\": 1}")),
        tips: Some(PgJson("{\"seen\": [\"a\", \"b\"]}")),
        explored_features: Some(PgJson("{\"cycles\": true}")),
        created_by: Some(USER_ALICE),
        updated_by: Some(USER_ALICE),
        workspace: WORKSPACE_ID,
    }
}

// ---------------------------------------------------------------------------
// Probe serializer (see serializers_record.py)
// ---------------------------------------------------------------------------

/// Field classes the Plane models do not put in a readable position.
#[derive(Clone, Debug)]
struct ProbeSerializer<'a> {
    id: Uuid,
    count: i64,
    ratio: PyDecimal,
    ratio_float: PyDecimal,
    flag: bool,
    name: &'a str,
    date: NaiveDate,
    time: NaiveTime,
    when: DateTime<Utc>,
    payload: PgJson<'a>,
    blob: Option<&'a [u8]>,
    nullable_text: Option<&'a str>,
}

const PROBE_FIELDS: &[&str] = &[
    "id",
    "count",
    "ratio",
    "ratio_float",
    "flag",
    "name",
    "date",
    "time",
    "when",
    "payload",
    "blob",
    "nullable_text",
];

impl DrfSerialize for ProbeSerializer<'_> {
    fn drf_write(
        &self,
        w: &mut plane_serializers::DrfWriter,
        ctx: &RenderCtx<'_>,
    ) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.id))?;
        obj.field_dyn("count", ctx, &self.count)?;
        obj.field_dyn("ratio", ctx, &DrfDecimal::new(self.ratio.clone(), 6, 2))?;
        obj.field_dyn(
            "ratio_float",
            ctx,
            &EncoderDecimal::Drf(self.ratio_float.clone()),
        )?;
        obj.field_dyn("flag", ctx, &self.flag)?;
        obj.field_dyn("name", ctx, &self.name)?;
        obj.field_dyn("date", ctx, &PyDate(self.date))?;
        obj.field_dyn("time", ctx, &PyTime(self.time))?;
        obj.field_dyn(
            "when",
            ctx,
            &plane_serializers::drf_datetime(self.when, ctx.tz()),
        )?;
        obj.field_json_dyn("payload", ctx, Some(self.payload))?;
        obj.field_dyn("blob", ctx, &self.blob.map(PyBase64))?;
        obj.field_dyn("nullable_text", ctx, &self.nullable_text)?;
        ctx.expansions()
            .write_injected(&mut obj, ctx, PROBE_FIELDS, FilterMode::None)?;
        obj.end();
        Ok(())
    }
}

fn probe() -> ProbeSerializer<'static> {
    ProbeSerializer {
        id: PROBE_ID,
        count: 7,
        ratio: PyDecimal::parse("12.30").expect("decimal"),
        ratio_float: PyDecimal::parse("12.30").expect("decimal"),
        flag: true,
        name: "Zo\u{eb} \"quoted\" \\ backslash \n newline \u{2028} line",
        date: NaiveDate::from_ymd_opt(2026, 11, 1).expect("date"),
        time: NaiveTime::from_hms_micro_opt(1, 2, 3, 456_789).expect("time"),
        when: utc(2026, 11, 1, 6, 30, 0) + chrono::TimeDelta::microseconds(123_400),
        payload: PgJson(
            "{\"z\": 1, \"a\": [1, 2.5e-05, \"\u{e9}\"], \"m\": {\"y\": true, \"b\": null}}",
        ),
        blob: Some(BINARY),
        nullable_text: None,
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

const USER_LITE: &str = "plane.app.serializers.user.UserLiteSerializer";
const USER_ME: &str = "plane.app.serializers.user.UserMeSerializer";
const WORKSPACE_LITE: &str = "plane.app.serializers.workspace.WorkspaceLiteSerializer";
const WORKSPACE: &str = "plane.app.serializers.workspace.WorkSpaceSerializer";
const WORKSPACE_MEMBER: &str = "plane.app.serializers.workspace.WorkSpaceMemberSerializer";
const PROJECT_LITE: &str = "plane.app.serializers.project.ProjectLiteSerializer";
const STATE: &str = "plane.app.serializers.state.StateSerializer";
const STATE_LITE: &str = "plane.app.serializers.state.StateLiteSerializer";
const LABEL: &str = "plane.app.serializers.issue.LabelSerializer";
const ISSUE: &str = "plane.app.serializers.issue.IssueSerializer";
const ISSUE_FLAT: &str = "plane.app.serializers.issue.IssueFlatSerializer";
const ISSUE_DETAIL: &str = "plane.app.serializers.issue.IssueDetailSerializer";
const ISSUE_STATE: &str = "plane.app.serializers.issue.IssueStateSerializer";
const CYCLE: &str = "plane.app.serializers.cycle.CycleSerializer";
const MODULE: &str = "plane.app.serializers.module.ModuleSerializer";
const V1_ISSUE_LITE: &str = "plane.api.serializers.issue.IssueLiteSerializer";
const V1_WORKSPACE_LITE: &str = "plane.api.serializers.workspace.WorkspaceLiteSerializer";
const PROBE: &str = "golden.GoldenProbeSerializer";

fn tz(name: &str) -> Tz {
    match name {
        "UTC" => chrono_tz::UTC,
        "Asia/Kolkata" => chrono_tz::Asia::Kolkata,
        "America/New_York" => chrono_tz::America::New_York,
        other => panic!("unknown timezone {other}"),
    }
}

fn entries<'a>(
    specs: &'a [serde_json::Value],
    nested_names: &'a [Vec<&'a str>],
) -> Vec<FieldEntry<'a>> {
    specs
        .iter()
        .zip(nested_names)
        .map(|(spec, names)| match spec {
            serde_json::Value::String(name) => FieldEntry::Name(name),
            serde_json::Value::Object(_) => FieldEntry::Nested {
                names: names.as_slice(),
                fields: &[],
            },
            other => panic!("unsupported fields entry {other}"),
        })
        .collect()
}

/// The dict-item form's keys, owned by the caller so `FieldEntry::Nested` can
/// borrow them.
fn nested_names(specs: &[serde_json::Value]) -> Vec<Vec<&str>> {
    specs
        .iter()
        .map(|spec| match spec {
            serde_json::Value::String(_) => Vec::new(),
            serde_json::Value::Object(map) => map.keys().map(String::as_str).collect(),
            other => panic!("unsupported fields entry {other}"),
        })
        .collect()
}

/// `expand=` values the handler would resolve. Only names the serializer
/// actually renders get a value; unknown names are ignored by DRF and by
/// [`Expansions::write_injected`].
fn expansions<'a>(
    record: &'a Record,
    requested: &[&'a str],
    fx: &'a Fixture,
) -> Vec<Expansion<'a>> {
    let mut values = Vec::new();
    for &name in requested {
        let value = match (record.serializer.as_str(), name) {
            (WORKSPACE, "owner") => DrfValue::Uuid(fx.workspace.owner),
            (WORKSPACE_MEMBER, "member") => DrfValue::Uuid(USER_ALICE),
            (WORKSPACE_MEMBER, "workspace") => DrfValue::nested(&fx.workspace_lite),
            (ISSUE | ISSUE_DETAIL | ISSUE_STATE, "state") => DrfValue::nested(&fx.state_lite),
            (ISSUE, "created_by") => DrfValue::nested(&fx.alice),
            (ISSUE | ISSUE_STATE, "labels") => {
                // The mapper points at the full `LabelSerializer`, not
                // `LabelLiteSerializer`.
                DrfValue::array(vec![DrfValue::nested(&fx.label)])
            }
            (ISSUE, "assignees") => DrfValue::array(vec![DrfValue::nested(&fx.alice)]),
            (ISSUE, "parent") => DrfValue::Null,
            (ISSUE | ISSUE_DETAIL, "issue_attachments") => {
                DrfValue::array(vec![DrfValue::nested(&fx.attachment)])
            }
            (ISSUE_STATE | MODULE, "project") => DrfValue::nested(&fx.project_lite),
            (MODULE, "created_by") => DrfValue::nested(&fx.alice),
            // `plane.api` base: `expand=` replaces a base field with
            // `<name>_id`; these annotations have no such attribute, so DRF
            // writes null. Anything else is not a field and adds nothing.
            (V1_ISSUE_LITE, "project_id") => DrfValue::Null,
            (V1_WORKSPACE_LITE, "name") => DrfValue::Null,
            _ => continue,
        };
        values.push(Expansion { name, value });
    }
    values
}

fn render_record(record: &Record, fx: &Fixture) -> Result<Vec<u8>, DrfError> {
    let zone = tz(&record.tz);
    let names = record.fields.as_deref().map(nested_names);
    let entries = match (record.fields.as_deref(), names.as_deref()) {
        (Some(specs), Some(names)) => Some(entries(specs, names)),
        _ => None,
    };
    let requested: Vec<&str> = record
        .expand
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(String::as_str)
        .collect();
    let values = expansions(record, &requested, fx);
    let ctx = RenderCtx::new(&zone, entries.as_deref(), &requested)
        .with_expansions(Expansions::new(&requested, &values));

    let serializer = record.serializer.as_str();
    match (serializer, record.subject.as_str(), record.many) {
        (USER_LITE, "user1", false) => plane_serializers::render(&fx.alice, &ctx),
        (USER_LITE, "user_empty", false) => plane_serializers::render(&fx.empty, &ctx),
        (USER_LITE, "user_bot", false) => plane_serializers::render(&fx.bot, &ctx),
        (USER_LITE, "user_pair", true) => {
            plane_serializers::render_list(&[fx.alice.clone(), fx.bob.clone()], &ctx)
        }
        (USER_ME, "user1", false) => plane_serializers::render(&fx.user_me, &ctx),
        (WORKSPACE_LITE, "workspace1", false) => {
            plane_serializers::render(&fx.workspace_lite, &ctx)
        }
        (WORKSPACE, "workspace1", false) => plane_serializers::render(&fx.workspace, &ctx),
        (WORKSPACE_MEMBER, "member1", false) => {
            plane_serializers::render(&workspace_member(fx), &ctx)
        }
        (PROJECT_LITE, "project1", false) => plane_serializers::render(&fx.project_lite, &ctx),
        (STATE, "state1", false) => plane_serializers::render(&fx.state, &ctx),
        (STATE, "state1_with_order", false) => {
            plane_serializers::render(&fx.state_with_order, &ctx)
        }
        (STATE, "state_pair", true) => {
            plane_serializers::render_list(&[fx.state.clone(), fx.state_triage.clone()], &ctx)
        }
        (STATE_LITE, "state1", false) => plane_serializers::render(&fx.state_lite, &ctx),
        (LABEL, "label1", false) => plane_serializers::render(&fx.label, &ctx),
        (ISSUE, "issue1", false) => plane_serializers::render(&fx.issue, &ctx),
        (ISSUE, "issue_pair", true) => {
            plane_serializers::render_list(&[fx.issue.clone(), fx.issue_child.clone()], &ctx)
        }
        (ISSUE_FLAT, "issue1", false) => plane_serializers::render(&fx.issue_flat, &ctx),
        (ISSUE_DETAIL, "issue1", false) => plane_serializers::render(&fx.issue_detail, &ctx),
        (ISSUE_STATE, "issue1", false) => plane_serializers::render(&issue_state(fx), &ctx),
        (CYCLE, "cycle1", false) => plane_serializers::render(&fx.cycle, &ctx),
        (MODULE, "module1", false) => plane_serializers::render(&fx.module, &ctx),
        (V1_ISSUE_LITE, "issue1", false) => plane_serializers::render(&fx.v1_issue_lite, &ctx),
        (V1_WORKSPACE_LITE, "workspace1", false) => {
            plane_serializers::render(&fx.v1_workspace_lite, &ctx)
        }
        (PROBE, "probe1", false) => plane_serializers::render(&probe(), &ctx),
        (serializer, subject, many) => panic!(
            "no fixture for ({serializer}, {subject}, many={many}) in case {}",
            record.case
        ),
    }
}

#[test]
fn golden_serializers() {
    let fx = fixture();
    let records = records();
    assert_eq!(records.len(), 106, "golden case count");

    let mut failures = Vec::new();
    for record in &records {
        let expected = record.out.as_bytes();
        match render_record(record, &fx) {
            Ok(actual) => {
                if actual != expected {
                    failures.push(format!(
                        "{}\n  expected: {}\n  actual:   {}",
                        record.case,
                        record.out,
                        String::from_utf8_lossy(&actual)
                    ));
                }
            }
            Err(error) => failures.push(format!("{}: {error}", record.case)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        records.len(),
        failures.join("\n")
    );
}

/// `many=True` on an empty list is `ListSerializer`, which renders `[]`.
#[test]
fn empty_list() {
    let ctx = RenderCtx::utc();
    let items: [UserLiteSerializer<'static>; 0] = [];
    let rendered = plane_serializers::render_list(&items, &ctx).expect("render");
    assert_eq!(rendered, b"[]");
}
