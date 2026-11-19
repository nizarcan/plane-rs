// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectViewSet`'s write actions (`plane/app/views/project/base.py`).
//!
//! | Route | Python |
//! | --- | --- |
//! | `POST /api/workspaces/{slug}/projects/` | `ProjectViewSet.create` (`:258-313`) |
//! | `PATCH /api/workspaces/{slug}/projects/{pk}/` | `ProjectViewSet.partial_update` (`:315-381`) |
//! | `DELETE …/projects/{pk}/` | `ProjectViewSet.destroy` (`:383-425`) |
//!
//! `PUT` is `rest_framework.mixins.UpdateModelMixin.update`, which
//! `BaseViewSet`'s guard refuses with 405 (`routes.json`'s
//! `viewset_guard.refused_methods`); this module does not register it.
//!
//! ## create (`:258-313`)
//!
//! `allow_permission([ADMIN, MEMBER], WORKSPACE)` is the runtime's; then
//! `Workspace.objects.get` → `ProjectSerializer` (`ProjectSerializer.create`:
//! INSERT `projects`, INSERT `project_identifiers`) → `ProjectMember` for the
//! caller (H32's `ProjectUserProperty` first) → the same pair for a different
//! `project_lead` → one `bulk_create` of the six `DEFAULT_STATES` (H19
//! bypassed: empty `slug`, fixed sequences, `created_by` set, `updated_by`
//! NULL, `is_triage` false) → re-fetch `get_queryset().filter(pk=…)` and answer
//! with `ProjectListSerializer` → `model_activity` (`current_instance=None`).
//!
//! ## partial_update (`:315-381`)
//!
//! Both admin `EXISTS` queries run before the 403 (the Python evaluates them
//! unconditionally) → `Workspace.get` → `Project.objects.get` → `intake_view =
//! request.data.get("inbox_view", project.intake_view)` → `current_instance =
//! json.dumps(ProjectSerializer(project).data, cls=DjangoJSONEncoder)` **before**
//! the update (it is the activity message's `current_instance`) → archived
//! project → 400 → `ProjectSerializer(project, data={**request.data,
//! "intake_view": intake_view}, partial=True)` → save (H09 identifier, and H09a
//! in one transaction on a timezone change) → when `intake_view` and no default
//! `Intake`, create `"{name} Intake"` → re-fetch → `model_activity` with
//! `current_instance` → 200.
//!
//! ## destroy (`:383-425`)
//!
//! `WorkspaceMember` admin **or** `ProjectMember` admin (Python short-circuits
//! the `or`); `Project.objects.get`; `project.delete()` (H03: `deleted_at`, a
//! full save = H09 identifier + H01 audit, then the `soft_delete_related_objects`
//! task); `webhook_activity` (`verb="deleted"`, `event_id` is the UUID itself);
//! `DeployBoard` and `UserFavorite` queryset deletes (H04: bare `deleted_at`);
//! 204.
//!
//! ## `current_instance`
//!
//! `ProjectSerializer(project).data` is a 38-key dict — the declared
//! `workspace_detail` and `inbox_view` first, then `Project._meta.fields` in
//! declaration order, then `_meta.forward_relations` — handed to
//! `json.dumps(..., cls=DjangoJSONEncoder)`: `", "`/`": "` separators,
//! `ensure_ascii=True`, UUIDs and datetimes through Django's encoder. The JSON
//! columns are re-encoded from the stored `jsonb` text with
//! [`plane_serde::transcode_json`], which is `json.loads` followed by
//! `json.dumps` (key order kept, Python float repr).
//!
//! ## Statement order
//!
//! Django runs without `ATOMIC_REQUESTS`, so each statement autocommits except
//! the timezone branch's `transaction.atomic()`. The handlers reproduce that
//! boundary; `common::client(ctx)` is the request's own (write) connection.

use chrono::{DateTime, Utc};
use deadpool_postgres::{Client, GenericClient};
use plane_core::error::ApiError;
use plane_db::models::project::Project;
use plane_db::models::workspace::Workspace;
use plane_db::{JsonText, hooks};
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyStr, Value as Json};
use uuid::Uuid;

use crate::common::asset_url::{self, AssetRef};
use crate::common::{client, lookup};
use crate::projects::base;
use crate::projects::queries;
use crate::write::fields::{self, CharOptions, Detail, Raw};
use crate::write::{self, Serializer};

// ---------------------------------------------------------------------------
// Model defaults (`plane/db/models/project.py:39-65`, `issue.py:47-88`)
// ---------------------------------------------------------------------------

/// `get_default_props()` for a `ProjectMember`'s `view_props`/`default_props`.
const MEMBER_PROPS: &str = r#"{"filters": {"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}, "display_filters": {"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}}"#;

/// `get_default_preferences()` (`project.py:64-65`).
const MEMBER_PREFERENCES: &str = r#"{"pages": {"block_display": true}, "navigation": {"default_tab": "work_items", "hide_in_more_menu": []}}"#;

/// `get_default_filters()` (`issue.py:47-58`).
const PUP_FILTERS: &str = r#"{"priority": null, "state": null, "state_group": null, "assignees": null, "created_by": null, "labels": null, "start_date": null, "target_date": null, "subscriber": null}"#;

/// `get_default_display_filters()` (`issue.py:61-70`).
const PUP_DISPLAY_FILTERS: &str = r#"{"group_by": null, "order_by": "-created_at", "type": null, "sub_issue": true, "show_empty_groups": true, "layout": "list", "calendar_date_range": ""}"#;

/// `get_default_display_properties()` (`issue.py:73-88`).
const PUP_DISPLAY_PROPERTIES: &str = r#"{"assignee": true, "attachment_count": true, "created_on": true, "due_date": true, "estimate": true, "key": true, "labels": true, "link": true, "priority": true, "start_date": true, "state": true, "sub_issue_count": true, "updated_on": true}"#;

/// `default_states` (`plane/db/models/state.py:24-62`).
struct DefaultState {
    name: &'static str,
    color: &'static str,
    sequence: f64,
    group: &'static str,
    default: bool,
}

const DEFAULT_STATES: [DefaultState; 6] = [
    DefaultState {
        name: "Backlog",
        color: "#60646C",
        sequence: 15000.0,
        group: "backlog",
        default: true,
    },
    DefaultState {
        name: "Todo",
        color: "#60646C",
        sequence: 25000.0,
        group: "unstarted",
        default: false,
    },
    DefaultState {
        name: "In Progress",
        color: "#F59E0B",
        sequence: 35000.0,
        group: "started",
        default: false,
    },
    DefaultState {
        name: "Done",
        color: "#46A758",
        sequence: 45000.0,
        group: "completed",
        default: false,
    },
    DefaultState {
        name: "Cancelled",
        color: "#9AA4BC",
        sequence: 55000.0,
        group: "cancelled",
        default: false,
    },
    DefaultState {
        name: "Triage",
        color: "#4E5355",
        sequence: 65000.0,
        group: "triage",
        default: false,
    },
];

/// `Project.timezone`'s choices: `pytz.common_timezones`
/// (`plane/db/models/project.py:117`). A `ChoiceField`, so an unknown zone is a
/// 400 `"…" is not a valid choice.`, not a 500.
const COMMON_TIMEZONES: &[&str] = &[
    "Africa/Abidjan",
    "Africa/Accra",
    "Africa/Addis_Ababa",
    "Africa/Algiers",
    "Africa/Asmara",
    "Africa/Bamako",
    "Africa/Bangui",
    "Africa/Banjul",
    "Africa/Bissau",
    "Africa/Blantyre",
    "Africa/Brazzaville",
    "Africa/Bujumbura",
    "Africa/Cairo",
    "Africa/Casablanca",
    "Africa/Ceuta",
    "Africa/Conakry",
    "Africa/Dakar",
    "Africa/Dar_es_Salaam",
    "Africa/Djibouti",
    "Africa/Douala",
    "Africa/El_Aaiun",
    "Africa/Freetown",
    "Africa/Gaborone",
    "Africa/Harare",
    "Africa/Johannesburg",
    "Africa/Juba",
    "Africa/Kampala",
    "Africa/Khartoum",
    "Africa/Kigali",
    "Africa/Kinshasa",
    "Africa/Lagos",
    "Africa/Libreville",
    "Africa/Lome",
    "Africa/Luanda",
    "Africa/Lubumbashi",
    "Africa/Lusaka",
    "Africa/Malabo",
    "Africa/Maputo",
    "Africa/Maseru",
    "Africa/Mbabane",
    "Africa/Mogadishu",
    "Africa/Monrovia",
    "Africa/Nairobi",
    "Africa/Ndjamena",
    "Africa/Niamey",
    "Africa/Nouakchott",
    "Africa/Ouagadougou",
    "Africa/Porto-Novo",
    "Africa/Sao_Tome",
    "Africa/Tripoli",
    "Africa/Tunis",
    "Africa/Windhoek",
    "America/Adak",
    "America/Anchorage",
    "America/Anguilla",
    "America/Antigua",
    "America/Araguaina",
    "America/Argentina/Buenos_Aires",
    "America/Argentina/Catamarca",
    "America/Argentina/Cordoba",
    "America/Argentina/Jujuy",
    "America/Argentina/La_Rioja",
    "America/Argentina/Mendoza",
    "America/Argentina/Rio_Gallegos",
    "America/Argentina/Salta",
    "America/Argentina/San_Juan",
    "America/Argentina/San_Luis",
    "America/Argentina/Tucuman",
    "America/Argentina/Ushuaia",
    "America/Aruba",
    "America/Asuncion",
    "America/Atikokan",
    "America/Bahia",
    "America/Bahia_Banderas",
    "America/Barbados",
    "America/Belem",
    "America/Belize",
    "America/Blanc-Sablon",
    "America/Boa_Vista",
    "America/Bogota",
    "America/Boise",
    "America/Cambridge_Bay",
    "America/Campo_Grande",
    "America/Cancun",
    "America/Caracas",
    "America/Cayenne",
    "America/Cayman",
    "America/Chicago",
    "America/Chihuahua",
    "America/Ciudad_Juarez",
    "America/Costa_Rica",
    "America/Creston",
    "America/Cuiaba",
    "America/Curacao",
    "America/Danmarkshavn",
    "America/Dawson",
    "America/Dawson_Creek",
    "America/Denver",
    "America/Detroit",
    "America/Dominica",
    "America/Edmonton",
    "America/Eirunepe",
    "America/El_Salvador",
    "America/Fort_Nelson",
    "America/Fortaleza",
    "America/Glace_Bay",
    "America/Goose_Bay",
    "America/Grand_Turk",
    "America/Grenada",
    "America/Guadeloupe",
    "America/Guatemala",
    "America/Guayaquil",
    "America/Guyana",
    "America/Halifax",
    "America/Havana",
    "America/Hermosillo",
    "America/Indiana/Indianapolis",
    "America/Indiana/Knox",
    "America/Indiana/Marengo",
    "America/Indiana/Petersburg",
    "America/Indiana/Tell_City",
    "America/Indiana/Vevay",
    "America/Indiana/Vincennes",
    "America/Indiana/Winamac",
    "America/Inuvik",
    "America/Iqaluit",
    "America/Jamaica",
    "America/Juneau",
    "America/Kentucky/Louisville",
    "America/Kentucky/Monticello",
    "America/Kralendijk",
    "America/La_Paz",
    "America/Lima",
    "America/Los_Angeles",
    "America/Lower_Princes",
    "America/Maceio",
    "America/Managua",
    "America/Manaus",
    "America/Marigot",
    "America/Martinique",
    "America/Matamoros",
    "America/Mazatlan",
    "America/Menominee",
    "America/Merida",
    "America/Metlakatla",
    "America/Mexico_City",
    "America/Miquelon",
    "America/Moncton",
    "America/Monterrey",
    "America/Montevideo",
    "America/Montserrat",
    "America/Nassau",
    "America/New_York",
    "America/Nome",
    "America/Noronha",
    "America/North_Dakota/Beulah",
    "America/North_Dakota/Center",
    "America/North_Dakota/New_Salem",
    "America/Nuuk",
    "America/Ojinaga",
    "America/Panama",
    "America/Paramaribo",
    "America/Phoenix",
    "America/Port-au-Prince",
    "America/Port_of_Spain",
    "America/Porto_Velho",
    "America/Puerto_Rico",
    "America/Punta_Arenas",
    "America/Rankin_Inlet",
    "America/Recife",
    "America/Regina",
    "America/Resolute",
    "America/Rio_Branco",
    "America/Santarem",
    "America/Santiago",
    "America/Santo_Domingo",
    "America/Sao_Paulo",
    "America/Scoresbysund",
    "America/Sitka",
    "America/St_Barthelemy",
    "America/St_Johns",
    "America/St_Kitts",
    "America/St_Lucia",
    "America/St_Thomas",
    "America/St_Vincent",
    "America/Swift_Current",
    "America/Tegucigalpa",
    "America/Thule",
    "America/Tijuana",
    "America/Toronto",
    "America/Tortola",
    "America/Vancouver",
    "America/Whitehorse",
    "America/Winnipeg",
    "America/Yakutat",
    "Antarctica/Casey",
    "Antarctica/Davis",
    "Antarctica/DumontDUrville",
    "Antarctica/Macquarie",
    "Antarctica/Mawson",
    "Antarctica/McMurdo",
    "Antarctica/Palmer",
    "Antarctica/Rothera",
    "Antarctica/Syowa",
    "Antarctica/Troll",
    "Antarctica/Vostok",
    "Arctic/Longyearbyen",
    "Asia/Aden",
    "Asia/Almaty",
    "Asia/Amman",
    "Asia/Anadyr",
    "Asia/Aqtau",
    "Asia/Aqtobe",
    "Asia/Ashgabat",
    "Asia/Atyrau",
    "Asia/Baghdad",
    "Asia/Bahrain",
    "Asia/Baku",
    "Asia/Bangkok",
    "Asia/Barnaul",
    "Asia/Beirut",
    "Asia/Bishkek",
    "Asia/Brunei",
    "Asia/Chita",
    "Asia/Choibalsan",
    "Asia/Colombo",
    "Asia/Damascus",
    "Asia/Dhaka",
    "Asia/Dili",
    "Asia/Dubai",
    "Asia/Dushanbe",
    "Asia/Famagusta",
    "Asia/Gaza",
    "Asia/Hebron",
    "Asia/Ho_Chi_Minh",
    "Asia/Hong_Kong",
    "Asia/Hovd",
    "Asia/Irkutsk",
    "Asia/Jakarta",
    "Asia/Jayapura",
    "Asia/Jerusalem",
    "Asia/Kabul",
    "Asia/Kamchatka",
    "Asia/Karachi",
    "Asia/Kathmandu",
    "Asia/Khandyga",
    "Asia/Kolkata",
    "Asia/Krasnoyarsk",
    "Asia/Kuala_Lumpur",
    "Asia/Kuching",
    "Asia/Kuwait",
    "Asia/Macau",
    "Asia/Magadan",
    "Asia/Makassar",
    "Asia/Manila",
    "Asia/Muscat",
    "Asia/Nicosia",
    "Asia/Novokuznetsk",
    "Asia/Novosibirsk",
    "Asia/Omsk",
    "Asia/Oral",
    "Asia/Phnom_Penh",
    "Asia/Pontianak",
    "Asia/Pyongyang",
    "Asia/Qatar",
    "Asia/Qostanay",
    "Asia/Qyzylorda",
    "Asia/Riyadh",
    "Asia/Sakhalin",
    "Asia/Samarkand",
    "Asia/Seoul",
    "Asia/Shanghai",
    "Asia/Singapore",
    "Asia/Srednekolymsk",
    "Asia/Taipei",
    "Asia/Tashkent",
    "Asia/Tbilisi",
    "Asia/Tehran",
    "Asia/Thimphu",
    "Asia/Tokyo",
    "Asia/Tomsk",
    "Asia/Ulaanbaatar",
    "Asia/Urumqi",
    "Asia/Ust-Nera",
    "Asia/Vientiane",
    "Asia/Vladivostok",
    "Asia/Yakutsk",
    "Asia/Yangon",
    "Asia/Yekaterinburg",
    "Asia/Yerevan",
    "Atlantic/Azores",
    "Atlantic/Bermuda",
    "Atlantic/Canary",
    "Atlantic/Cape_Verde",
    "Atlantic/Faroe",
    "Atlantic/Madeira",
    "Atlantic/Reykjavik",
    "Atlantic/South_Georgia",
    "Atlantic/St_Helena",
    "Atlantic/Stanley",
    "Australia/Adelaide",
    "Australia/Brisbane",
    "Australia/Broken_Hill",
    "Australia/Darwin",
    "Australia/Eucla",
    "Australia/Hobart",
    "Australia/Lindeman",
    "Australia/Lord_Howe",
    "Australia/Melbourne",
    "Australia/Perth",
    "Australia/Sydney",
    "Canada/Atlantic",
    "Canada/Central",
    "Canada/Eastern",
    "Canada/Mountain",
    "Canada/Newfoundland",
    "Canada/Pacific",
    "Europe/Amsterdam",
    "Europe/Andorra",
    "Europe/Astrakhan",
    "Europe/Athens",
    "Europe/Belgrade",
    "Europe/Berlin",
    "Europe/Bratislava",
    "Europe/Brussels",
    "Europe/Bucharest",
    "Europe/Budapest",
    "Europe/Busingen",
    "Europe/Chisinau",
    "Europe/Copenhagen",
    "Europe/Dublin",
    "Europe/Gibraltar",
    "Europe/Guernsey",
    "Europe/Helsinki",
    "Europe/Isle_of_Man",
    "Europe/Istanbul",
    "Europe/Jersey",
    "Europe/Kaliningrad",
    "Europe/Kirov",
    "Europe/Kyiv",
    "Europe/Lisbon",
    "Europe/Ljubljana",
    "Europe/London",
    "Europe/Luxembourg",
    "Europe/Madrid",
    "Europe/Malta",
    "Europe/Mariehamn",
    "Europe/Minsk",
    "Europe/Monaco",
    "Europe/Moscow",
    "Europe/Oslo",
    "Europe/Paris",
    "Europe/Podgorica",
    "Europe/Prague",
    "Europe/Riga",
    "Europe/Rome",
    "Europe/Samara",
    "Europe/San_Marino",
    "Europe/Sarajevo",
    "Europe/Saratov",
    "Europe/Simferopol",
    "Europe/Skopje",
    "Europe/Sofia",
    "Europe/Stockholm",
    "Europe/Tallinn",
    "Europe/Tirane",
    "Europe/Ulyanovsk",
    "Europe/Vaduz",
    "Europe/Vatican",
    "Europe/Vienna",
    "Europe/Vilnius",
    "Europe/Volgograd",
    "Europe/Warsaw",
    "Europe/Zagreb",
    "Europe/Zurich",
    "GMT",
    "Indian/Antananarivo",
    "Indian/Chagos",
    "Indian/Christmas",
    "Indian/Cocos",
    "Indian/Comoro",
    "Indian/Kerguelen",
    "Indian/Mahe",
    "Indian/Maldives",
    "Indian/Mauritius",
    "Indian/Mayotte",
    "Indian/Reunion",
    "Pacific/Apia",
    "Pacific/Auckland",
    "Pacific/Bougainville",
    "Pacific/Chatham",
    "Pacific/Chuuk",
    "Pacific/Easter",
    "Pacific/Efate",
    "Pacific/Fakaofo",
    "Pacific/Fiji",
    "Pacific/Funafuti",
    "Pacific/Galapagos",
    "Pacific/Gambier",
    "Pacific/Guadalcanal",
    "Pacific/Guam",
    "Pacific/Honolulu",
    "Pacific/Kanton",
    "Pacific/Kiritimati",
    "Pacific/Kosrae",
    "Pacific/Kwajalein",
    "Pacific/Majuro",
    "Pacific/Marquesas",
    "Pacific/Midway",
    "Pacific/Nauru",
    "Pacific/Niue",
    "Pacific/Norfolk",
    "Pacific/Noumea",
    "Pacific/Pago_Pago",
    "Pacific/Palau",
    "Pacific/Pitcairn",
    "Pacific/Pohnpei",
    "Pacific/Port_Moresby",
    "Pacific/Rarotonga",
    "Pacific/Saipan",
    "Pacific/Tahiti",
    "Pacific/Tarawa",
    "Pacific/Tongatapu",
    "Pacific/Wake",
    "Pacific/Wallis",
    "US/Alaska",
    "US/Arizona",
    "US/Central",
    "US/Eastern",
    "US/Hawaii",
    "US/Mountain",
    "US/Pacific",
    "UTC",
];

// ---------------------------------------------------------------------------
// The body
// ---------------------------------------------------------------------------

/// The `ProjectSerializer` fields this port reads, in `self.fields` order.
///
/// `ModelSerializer` derives that order from `fields = "__all__"`: `id`, the
/// declared fields (`workspace_detail`, `inbox_view`), `_meta.fields`
/// (non-relational, declaration order), then `_meta.forward_relations`. The
/// read-only ones (`id`, `created_at`, `updated_at`, `workspace`,
/// `deleted_at`, `created_by`, `updated_by`, the two declared fields) never
/// read the body, so what remains starts at `name` and ends at `default_state`.
#[derive(Debug, Default)]
struct Body {
    name: Option<String>,
    description: Option<String>,
    description_text: Option<JsonText>,
    description_html: Option<JsonText>,
    network: Option<i16>,
    identifier: Option<String>,
    emoji: Option<String>,
    icon_prop: Option<JsonText>,
    module_view: Option<bool>,
    cycle_view: Option<bool>,
    issue_views_view: Option<bool>,
    page_view: Option<bool>,
    intake_view: Option<bool>,
    is_time_tracking_enabled: Option<bool>,
    is_issue_type_enabled: Option<bool>,
    guest_view_all_features: Option<bool>,
    cover_image: Option<String>,
    archive_in: Option<i32>,
    close_in: Option<i32>,
    logo_props: Option<JsonText>,
    archived_at: Option<DateTime<Utc>>,
    timezone: Option<String>,
    external_source: Option<String>,
    external_id: Option<String>,
    default_assignee: Option<Uuid>,
    project_lead: Option<Uuid>,
    cover_image_asset: Option<Uuid>,
    estimate: Option<Uuid>,
    default_state: Option<Uuid>,
}

/// `ProjectSerializer(data=…).to_internal_value`, plus `validate_<field>` and
/// the first half of `validate()` (the membership check).
///
/// Every value that exists in `validated_data` is recorded in the serializer
/// (`Validated::contains` tells "absent" from "explicit null"); field errors
/// are pushed in `self.fields` order, and `validate()` only runs when the field
/// pass produced none.
// The field parsers have to run in serializer order and each one records its
// own value and error; building the struct in one initializer would reorder
// the calls.
#[allow(clippy::field_reassign_with_default)]
async fn read_body(
    serializer: &mut Serializer<'_>,
    client: &Client,
    workspace_id: Uuid,
    exclude: Option<Uuid>,
    instance: Option<&Project>,
    injected_intake: Option<Raw<'_>>,
) -> Result<Body, ViewError> {
    let mut body = Body::default();

    // name (#9). `CharField(max_length=255)`, required.
    body.name = serializer.char_field(
        "name",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if body.name.is_none() && serializer.missing_required("name") {
        serializer.push_error("name", Detail::required());
    }
    if let Some(name) = body.name.as_deref() {
        validate_field(client, workspace_id, "name", name, exclude, serializer).await?;
    }

    // description (#10). `TextField(blank=True)`: not nullable.
    body.description = serializer.char_field(
        "description",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    body.description_text = serializer.json_field("description_text", true);
    body.description_html = serializer.json_field("description_html", true);
    body.network = serializer.integer_choice_field("network", &[0, 2]);

    // identifier (#14). `CharField(max_length=12)`, required.
    body.identifier = serializer.char_field(
        "identifier",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(12),
        },
    );
    if body.identifier.is_none() && serializer.missing_required("identifier") {
        serializer.push_error("identifier", Detail::required());
    }
    if let Some(identifier) = body.identifier.as_deref() {
        validate_field(
            client,
            workspace_id,
            "identifier",
            identifier,
            exclude,
            serializer,
        )
        .await?;
    }

    body.emoji = serializer.char_field("emoji", CharOptions::nullable().with_max_length(255));
    body.icon_prop = serializer.json_field("icon_prop", true);
    body.module_view = serializer.bool_field("module_view");
    body.cycle_view = serializer.bool_field("cycle_view");
    body.issue_views_view = serializer.bool_field("issue_views_view");
    body.page_view = serializer.bool_field("page_view");
    // The view injects `intake_view`; create reads it from the body.
    body.intake_view = match injected_intake {
        Some(raw) => serializer.bool_field_value("intake_view", raw),
        None => serializer.bool_field("intake_view"),
    };
    body.is_time_tracking_enabled = serializer.bool_field("is_time_tracking_enabled");
    body.is_issue_type_enabled = serializer.bool_field("is_issue_type_enabled");
    body.guest_view_all_features = serializer.bool_field("guest_view_all_features");
    body.cover_image = serializer.char_field("cover_image", CharOptions::nullable());
    body.archive_in = serializer.field("archive_in", false, integer_0_12);
    body.close_in = serializer.field("close_in", false, integer_0_12);
    body.logo_props = serializer.json_field("logo_props", false);
    body.archived_at = serializer.datetime_field("archived_at", true);
    body.timezone = serializer.field("timezone", false, timezone_choice);
    body.external_source = serializer.char_field(
        "external_source",
        CharOptions::nullable().with_max_length(255),
    );
    body.external_id =
        serializer.char_field("external_id", CharOptions::nullable().with_max_length(255));

    // `_meta.forward_relations`, in order: each is a `PrimaryKeyRelatedField`
    // whose queryset lookup runs during field parsing.
    body.default_assignee = related_field(
        serializer,
        client,
        "default_assignee",
        queries::RELATED_USER_EXISTS.sql,
    )
    .await?;
    body.project_lead = related_field(
        serializer,
        client,
        "project_lead",
        queries::RELATED_USER_EXISTS.sql,
    )
    .await?;
    body.cover_image_asset = related_field(
        serializer,
        client,
        "cover_image_asset",
        queries::RELATED_FILE_ASSET_EXISTS.sql,
    )
    .await?;
    body.estimate = related_field(
        serializer,
        client,
        "estimate",
        queries::RELATED_ESTIMATE_EXISTS.sql,
    )
    .await?;
    body.default_state = related_field(
        serializer,
        client,
        "default_state",
        queries::RELATED_STATE_EXISTS.sql,
    )
    .await?;

    // `validate()` (`serializers/project.py:84-102`). It runs only after the
    // field pass succeeded, and raises on the first failing field.
    if !serializer.has_errors() {
        for (field, member) in [
            ("project_lead", body.project_lead),
            ("default_assignee", body.default_assignee),
        ] {
            let Some(member) = member else { continue };
            // An unchanged assignment may be resent after the user left.
            let unchanged = instance.is_some_and(|project| {
                if field == "project_lead" {
                    project.project_lead_id == Some(member)
                } else {
                    project.default_assignee_id == Some(member)
                }
            });
            if unchanged {
                continue;
            }
            if !workspace_member_exists(client, workspace_id, member).await? {
                serializer.push_error(
                    field,
                    Detail::new(
                        "The provided user is not a member of this workspace.",
                        "invalid",
                    ),
                );
                break;
            }
        }
    }

    Ok(body)
}

/// `{**request.data}` (`project/base.py:262`, `:352`): a body that is not a
/// mapping raises `TypeError` inside the view, which Plane's family
/// `handle_exception` renders as its generic 500 (`views/base.py:235-239`).
///
/// A form body is a `QueryDict`, which is a mapping, so it passes.
fn spread_body(data: &Data) -> Result<(), ViewError> {
    match data {
        Data::Json(Json::Dict(_)) | Data::Form(_) => Ok(()),
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not a mapping",
            python_type_name(other)
        ))),
    }
}

fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

/// A `PrimaryKeyRelatedField`: parse the pk, then `queryset.get(pk=…)`.
async fn related_field(
    serializer: &mut Serializer<'_>,
    client: &Client,
    name: &'static str,
    query: &str,
) -> Result<Option<Uuid>, ViewError> {
    let Some(value) = serializer.field(name, true, fields::uuid) else {
        return Ok(None);
    };
    let exists = client
        .query_opt(query, &[&value])
        .await
        .map_err(ViewError::from)?
        .is_some();
    if exists {
        return Ok(Some(value));
    }
    // `Invalid pk "{pk_value}" - object does not exist.` — `pk_value` is the
    // input, which for a JSON body is the raw string.
    let raw = serializer
        .raw_text(name)
        .unwrap_or_else(|| value.to_string());
    serializer.reject(
        name,
        Detail::new(
            format!("Invalid pk \"{raw}\" - object does not exist."),
            "does_not_exist",
        ),
    );
    Ok(None)
}

/// `IntegerField(min_value=0, max_value=12)` (`archive_in`, `close_in`).
fn integer_0_12(raw: Raw<'_>) -> Result<i32, Detail> {
    let options = fields::IntegerOptions {
        min_value: Some(0),
        max_value: Some(12),
    };
    Ok(fields::integer(raw, &options)? as i32)
}

/// `ChoiceField(TIMEZONE_CHOICES)`: `str(data)` against the choice keys.
fn timezone_choice(raw: Raw<'_>) -> Result<String, Detail> {
    let text = match raw {
        Raw::Text(text) => text.to_owned(),
        Raw::Json(value) => python_str(value),
    };
    if COMMON_TIMEZONES.contains(&text.as_str()) {
        Ok(text)
    } else {
        Err(Detail::new(
            format!("\"{text}\" is not a valid choice."),
            "invalid_choice",
        ))
    }
}

/// `validate_name`/`validate_identifier` (`serializers/project.py:36-65`).
async fn validate_field(
    client: &Client,
    workspace_id: Uuid,
    field: &'static str,
    value: &str,
    exclude: Option<Uuid>,
    serializer: &mut Serializer<'_>,
) -> Result<(), ViewError> {
    // `FORBIDDEN_IDENTIFIER_CHARS_PATTERN` (`project.py:150`): `re.match` with
    // `.*` around the class → "contains any of these".
    const FORBIDDEN: &str = "&+,:;$^}{*}{=?@#|'<>.()%!-";
    if value.chars().any(|character| FORBIDDEN.contains(character)) {
        serializer.push_error(
            field,
            Detail::new(
                if field == "name" {
                    "PROJECT_NAME_CANNOT_CONTAIN_SPECIAL_CHARACTERS"
                } else {
                    "PROJECT_IDENTIFIER_CANNOT_CONTAIN_SPECIAL_CHARACTERS"
                },
                "invalid",
            ),
        );
        return Ok(());
    }
    let sql = if field == "name" {
        "SELECT 1 FROM projects WHERE name = $1 AND workspace_id = $2 \
         AND deleted_at IS NULL AND ($3::uuid IS NULL OR id <> $3) LIMIT 1"
    } else {
        "SELECT 1 FROM projects WHERE identifier = $1 AND workspace_id = $2 \
         AND deleted_at IS NULL AND ($3::uuid IS NULL OR id <> $3) LIMIT 1"
    };
    let exists = client
        .query_opt(sql, &[&value, &workspace_id, &exclude])
        .await
        .map_err(ViewError::from)?
        .is_some();
    if exists {
        serializer.push_error(
            field,
            Detail::new(
                if field == "name" {
                    "PROJECT_NAME_ALREADY_EXIST"
                } else {
                    "PROJECT_IDENTIFIER_ALREADY_EXIST"
                },
                "invalid",
            ),
        );
    }
    Ok(())
}

async fn workspace_member_exists(
    client: &Client,
    workspace_id: Uuid,
    member: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            queries::WORKSPACE_MEMBER_EXISTS.sql,
            &[&workspace_id, &member],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

/// `ProjectViewSet.create` (`plane/app/views/project/base.py:258-313`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`:260`).
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;

        spread_body(ctx.data()?)?;
        // `data={**request.data}` (`:263`): a form body expands to `{key: [values]}`
        // before the serializer sees it.
        let mut serializer = Serializer::new_expanded(ctx, false)?;
        let mut body = read_body(&mut serializer, client, workspace.id, None, None, None).await?;
        if let Err(errors) = serializer.finish() {
            return Ok(errors.into_response());
        }
        // `validate()`'s HTML branch (`:104-112`).
        body.description_html = match sanitize_html(&body.description_html)? {
            Ok(value) => value,
            Err(response) => return Ok(response),
        };

        // `ProjectSerializer.create` (`serializers/project.py:107-113`) with
        // `Project(**validated_data, workspace_id=…)` defaults, then H08.
        let name = body
            .name
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?;
        let identifier = body
            .identifier
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated identifier vanished".to_owned()))?;
        let prepared = hooks::project::before_insert(
            &identifier,
            body.timezone.as_deref(),
            &workspace.timezone,
        );
        let now = write::now();
        let audit = hooks::audit::Audit::on_create(Some(user));
        let project = Project {
            created_at: now,
            updated_at: now,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            deleted_at: None,
            id: Uuid::new_v4(),
            name,
            description: body.description.clone().unwrap_or_default(),
            description_text: body.description_text.clone(),
            description_html: body.description_html.clone(),
            network: body.network.unwrap_or(2),
            workspace_id: workspace.id,
            identifier: prepared.identifier,
            default_assignee_id: body.default_assignee,
            project_lead_id: body.project_lead,
            emoji: body.emoji.clone(),
            icon_prop: body.icon_prop.clone(),
            module_view: body.module_view.unwrap_or(false),
            cycle_view: body.cycle_view.unwrap_or(false),
            issue_views_view: body.issue_views_view.unwrap_or(false),
            page_view: body.page_view.unwrap_or(true),
            intake_view: body.intake_view.unwrap_or(false),
            is_time_tracking_enabled: body.is_time_tracking_enabled.unwrap_or(false),
            is_issue_type_enabled: body.is_issue_type_enabled.unwrap_or(false),
            guest_view_all_features: body.guest_view_all_features.unwrap_or(false),
            cover_image: body.cover_image.clone(),
            cover_image_asset_id: body.cover_image_asset,
            estimate_id: body.estimate,
            archive_in: body.archive_in.unwrap_or(0),
            close_in: body.close_in.unwrap_or(0),
            logo_props: body
                .logo_props
                .clone()
                .unwrap_or_else(|| JsonText::new("{}")),
            default_state_id: body.default_state,
            archived_at: body.archived_at,
            timezone: prepared.timezone,
            external_source: body.external_source.clone(),
            external_id: body.external_id.clone(),
        };
        insert_project(client, &project).await?;
        insert_identifier(client, &project, now).await?;

        // `ProjectMember.objects.create(...)` for the caller (`:267-271`), then
        // the same pair for a different `project_lead` (`:276-280`).
        insert_member(client, &workspace, &project, user, 20, user, now).await?;
        if let Some(lead) = project.project_lead_id
            && lead != user
        {
            insert_member(client, &workspace, &project, lead, 20, user, now).await?;
        }

        // `State.objects.bulk_create(DEFAULT_STATES)` (`:282-296`).
        insert_default_states(client, &workspace, &project, user, now).await?;

        // `self.get_queryset().filter(pk=…).first()` (`:298`).
        let Some(row) = base::load_one(ctx, project.id).await? else {
            return Err(ViewError::Unknown(
                "the created project vanished".to_owned(),
            ));
        };
        // `model_activity.delay(...)` (`:300-309`) — `current_instance=None`.
        publish_activity(ctx, slug, project.id, None, user).await?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        base::render_project_row(&row, &timezone, 201)
    })
}

// ---------------------------------------------------------------------------
// partial_update
// ---------------------------------------------------------------------------

/// `ProjectViewSet.partial_update` (`plane/app/views/project/base.py:315-381`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = ctx.kwarg("pk").ok_or(ViewError::Key)?;
        let project_id = lookup::uuid_kwarg("pk", pk).map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `:317-337`: both queries run, then the 403.
        let workspace_admin = is_workspace_admin(client, slug, user).await?;
        let project_admin = is_project_admin(client, slug, project_id, user).await?;
        if !workspace_admin && !project_admin {
            return Ok(write::error_body(
                403,
                "You don't have the required permissions.",
            ));
        }
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;
        let project = get_project(client, project_id, slug).await?;

        // `intake_view = request.data.get("inbox_view", project.intake_view)`
        // (`:342`).
        let data = ctx.data()?;
        spread_body(data)?;
        let default_intake = Json::Bool(project.intake_view);
        let injected_intake = match data {
            Data::Json(Json::Dict(object)) => match object.get("inbox_view") {
                Some(value) => Raw::Json(value),
                None => Raw::Json(&default_intake),
            },
            other => match other.get("inbox_view") {
                Some(text) => Raw::Text(text),
                None => Raw::Json(&default_intake),
            },
        };
        // `current_instance = json.dumps(ProjectSerializer(project).data, …)`
        // (`:343`) — before the update.
        let current_instance = current_instance_json(client, &project, &workspace).await?;

        // `:344-348`.
        if project.archived_at.is_some() {
            return Ok(write::error_body(
                400,
                "Archived projects cannot be updated",
            ));
        }

        // `ProjectSerializer(project, data={**request.data, "intake_view": …},
        // partial=True)` (`:350-355`): a form body expands to `{key: [values]}`
        // before the serializer sees it.
        let mut serializer = Serializer::new_expanded(ctx, true)?;
        let mut body = read_body(
            &mut serializer,
            client,
            workspace.id,
            Some(project_id),
            Some(&project),
            Some(injected_intake),
        )
        .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        body.description_html = match sanitize_html(&body.description_html)? {
            Ok(value) => value,
            Err(response) => return Ok(response),
        };

        // `ModelSerializer.update`: assign the validated fields.
        let mut updated = project.clone();
        if let Some(name) = body.name.clone() {
            updated.name = name;
        }
        if let Some(description) = body.description.clone() {
            updated.description = description;
        }
        if values.contains("description_text") {
            updated.description_text = body.description_text.clone();
        }
        if values.contains("description_html") {
            updated.description_html = body.description_html.clone();
        }
        if let Some(network) = body.network {
            updated.network = network;
        }
        if let Some(identifier) = body.identifier.clone() {
            updated.identifier = identifier;
        }
        if values.contains("emoji") {
            updated.emoji = body.emoji.clone();
        }
        if values.contains("icon_prop") {
            updated.icon_prop = body.icon_prop.clone();
        }
        macro_rules! apply_bool {
            ($($field:ident),* $(,)?) => {
                $(
                    if let Some(value) = body.$field {
                        updated.$field = value;
                    }
                )*
            };
        }
        apply_bool!(
            module_view,
            cycle_view,
            issue_views_view,
            page_view,
            is_time_tracking_enabled,
            is_issue_type_enabled,
            guest_view_all_features,
        );
        // The injected value is always in `validated_data`.
        if let Some(value) = values.bool("intake_view") {
            updated.intake_view = value;
        }
        if values.contains("cover_image") {
            updated.cover_image = body.cover_image.clone();
        }
        if let Some(archive_in) = body.archive_in {
            updated.archive_in = archive_in;
        }
        if let Some(close_in) = body.close_in {
            updated.close_in = close_in;
        }
        if let Some(logo_props) = body.logo_props.clone() {
            updated.logo_props = logo_props;
        }
        if values.contains("archived_at") {
            updated.archived_at = body.archived_at;
        }
        if let Some(timezone) = body.timezone.clone() {
            updated.timezone = timezone;
        }
        if values.contains("external_source") {
            updated.external_source = body.external_source.clone();
        }
        if values.contains("external_id") {
            updated.external_id = body.external_id.clone();
        }
        if values.contains("default_assignee") {
            updated.default_assignee_id = body.default_assignee;
        }
        if values.contains("project_lead") {
            updated.project_lead_id = body.project_lead;
        }
        if values.contains("cover_image_asset") {
            updated.cover_image_asset_id = body.cover_image_asset;
        }
        if values.contains("estimate") {
            updated.estimate_id = body.estimate;
        }
        if values.contains("default_state") {
            updated.default_state_id = body.default_state;
        }

        // H09 identifier normalisation and H01, then H09a if the zone changed
        // (`project.py:196-227`).
        updated.identifier = hooks::project::before_update(&updated.identifier);
        let now = write::now();
        updated.updated_at = now;
        updated.updated_by_id = hooks::audit::Audit::on_update(Some(user));
        let timezone_changed = body.timezone.is_some() && updated.timezone != project.timezone;
        if timezone_changed {
            let old_zone = hooks::project::parse_timezone(&project.timezone).ok_or_else(|| {
                ViewError::Unknown(format!("UnknownTimeZoneError: {}", project.timezone))
            })?;
            let new_zone = hooks::project::parse_timezone(&updated.timezone).ok_or_else(|| {
                ViewError::Unknown(format!("UnknownTimeZoneError: {}", updated.timezone))
            })?;
            // `transaction.atomic()`: the request's connection only exposes
            // `&Client`, so the boundary is explicit (BEGIN / COMMIT / ROLLBACK).
            client
                .batch_execute("BEGIN")
                .await
                .map_err(ViewError::from)?;
            let outcome = async {
                update_project(client, &updated).await?;
                hooks::project::retime_intake_snoozes_with(client, project_id, old_zone, new_zone)
                    .await
                    .map_err(ViewError::from)
            }
            .await;
            match outcome {
                Ok(()) => client
                    .batch_execute("COMMIT")
                    .await
                    .map_err(ViewError::from)?,
                Err(error) => {
                    let _ = client.batch_execute("ROLLBACK").await;
                    return Err(error);
                }
            }
        } else {
            update_project(client, &updated).await?;
        }

        // `:359-367`: a default Intake when `inbox_view` became true. The
        // Python tests the *raw* value's truthiness.
        if py_truthy(&injected_intake) && !default_intake_exists(client, project_id).await? {
            insert_intake(client, &workspace, &updated, user, now).await?;
        }

        let Some(row) = base::load_one(ctx, project_id).await? else {
            return Err(ViewError::Unknown(
                "the updated project vanished".to_owned(),
            ));
        };
        publish_activity(ctx, slug, project_id, Some(current_instance), user).await?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        base::render_project_row(&row, &timezone, 200)
    })
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

/// `ProjectViewSet.destroy` (`plane/app/views/project/base.py:383-425`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = ctx.kwarg("pk").ok_or(ViewError::Key)?;
        let project_id = lookup::uuid_kwarg("pk", pk).map_err(ViewError::from)?;
        let user = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `:384-398`: Python's `or` short-circuits.
        if !is_workspace_admin(client, slug, user).await?
            && !is_project_admin(client, slug, project_id, user).await?
        {
            return Ok(write::error_body(
                403,
                "You don't have the required permissions.",
            ));
        }
        // `Project.objects.get(pk=pk, workspace__slug=slug)` (`:399`).
        let project = get_project(client, project_id, slug).await?;

        // `project.delete()` (`:400`): H03 + H09 + H01, then the cascade task.
        let now = write::now();
        let identifier = hooks::project::before_update(&project.identifier);
        let statement = client
            .prepare_cached(queries::PROJECT_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &hooks::audit::Audit::on_update(Some(user)),
                    &identifier,
                    &project_id,
                ],
            )
            .await
            .map_err(ViewError::from)?;
        write::publish_soft_delete_cascade(ctx, "db", "project", project_id).await?;

        // `webhook_activity.delay(...)` (`:401-413`), `event_id` is the UUID.
        let origin = base_host(ctx)?;
        let task = plane_jobs::celery::task_name::WEBHOOK_ACTIVITY;
        let message = plane_jobs::celery::tasks::webhook_activity(
            plane_jobs::celery::tasks::WebhookActivityCall {
                event: "project".to_owned(),
                verb: "deleted".to_owned(),
                field: None,
                old_value: plane_jobs::celery::args::TaskArg::Null,
                new_value: plane_jobs::celery::args::TaskArg::Null,
                actor_id: user,
                slug: slug.to_owned(),
                current_site: origin,
                event_id: plane_jobs::celery::args::TaskArg::uuid(project_id),
                old_identifier: None,
                new_identifier: None,
            },
        );
        crate::common::jobs::dispatch_task(
            ctx,
            task,
            &message,
            plane_jobs::queue::Job::from_message(&message),
        )
        .await?;

        // `DeployBoard.objects.filter(...).delete()` and
        // `UserFavorite.objects.filter(...).delete()` (`:415-418`) — H04.
        let statement = client
            .prepare_cached(queries::PROJECT_DEPLOY_BOARDS_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&now, &slug, &project_id])
            .await
            .map_err(ViewError::from)?;
        let statement = client
            .prepare_cached(queries::PROJECT_FAVORITES_SOFT_DELETE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(&statement, &[&now, &slug, &project_id])
            .await
            .map_err(ViewError::from)?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

async fn insert_project(client: &Client, project: &Project) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &project.created_at,
                &project.updated_at,
                &project.created_by_id,
                &project.updated_by_id,
                &project.deleted_at,
                &project.id,
                &project.name,
                &project.description,
                &project.description_text,
                &project.description_html,
                &project.network,
                &project.workspace_id,
                &project.identifier,
                &project.default_assignee_id,
                &project.project_lead_id,
                &project.emoji,
                &project.icon_prop,
                &project.module_view,
                &project.cycle_view,
                &project.issue_views_view,
                &project.page_view,
                &project.intake_view,
                &project.is_time_tracking_enabled,
                &project.is_issue_type_enabled,
                &project.guest_view_all_features,
                &project.cover_image,
                &project.cover_image_asset_id,
                &project.estimate_id,
                &project.archive_in,
                &project.close_in,
                &project.logo_props,
                &project.default_state_id,
                &project.archived_at,
                &project.timezone,
                &project.external_source,
                &project.external_id,
            ],
        )
        .await;
    map_write_error(result)
}

async fn update_project<C>(client: &C, project: &Project) -> Result<(), ViewError>
where
    C: GenericClient + ?Sized,
{
    let statement = client
        .prepare_cached(queries::PROJECT_UPDATE.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &project.created_at,
                &project.updated_at,
                &project.created_by_id,
                &project.updated_by_id,
                &project.deleted_at,
                &project.name,
                &project.description,
                &project.description_text,
                &project.description_html,
                &project.network,
                &project.workspace_id,
                &project.identifier,
                &project.default_assignee_id,
                &project.project_lead_id,
                &project.emoji,
                &project.icon_prop,
                &project.module_view,
                &project.cycle_view,
                &project.issue_views_view,
                &project.page_view,
                &project.intake_view,
                &project.is_time_tracking_enabled,
                &project.is_issue_type_enabled,
                &project.guest_view_all_features,
                &project.cover_image,
                &project.cover_image_asset_id,
                &project.estimate_id,
                &project.archive_in,
                &project.close_in,
                &project.logo_props,
                &project.default_state_id,
                &project.archived_at,
                &project.timezone,
                &project.external_source,
                &project.external_id,
                &project.id,
            ],
        )
        .await;
    map_write_error(result)
}

/// `django.db.IntegrityError` is a 400 (`plane/app/views/base.py:210-214`); any
/// other database error is the family's 500.
fn map_write_error(result: Result<u64, tokio_postgres::Error>) -> Result<(), ViewError> {
    match result {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.code(),
                Some(
                    &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
                        | &tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION
                )
            ) =>
        {
            Err(ViewError::Integrity)
        }
        Err(error) => Err(ViewError::from(error)),
    }
}

async fn insert_identifier(
    client: &Client,
    project: &Project,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_IDENTIFIER_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &now,
                &now,
                // `ProjectIdentifier(AuditModel)` has no `BaseModel.save`, so
                // `crum` never fills `created_by`.
                &None::<Uuid>,
                &None::<Uuid>,
                &project.workspace_id,
                &project.id,
                &project.identifier,
            ],
        )
        .await;
    map_write_error(result)
}

async fn insert_member(
    client: &Client,
    workspace: &Workspace,
    project: &Project,
    member: Uuid,
    role: i16,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    // H32 first: the member's `ProjectUserProperty`.
    let property = hooks::project_member::before_insert(client, workspace.id, project.id, member)
        .await
        .map_err(ViewError::from)?;
    let statement = client
        .prepare_cached(queries::PROJECT_USER_PROPERTY_MEMBER_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Some(user),
                &Uuid::new_v4(),
                &property.project_id,
                &property.workspace_id,
                &property.user_id,
                &JsonText::new(PUP_FILTERS),
                &JsonText::new(PUP_DISPLAY_FILTERS),
                &JsonText::new(PUP_DISPLAY_PROPERTIES),
                &JsonText::new(MEMBER_PREFERENCES),
                &property.sort_order,
            ],
        )
        .await;
    map_write_error(result)?;

    let statement = client
        .prepare_cached(queries::PROJECT_MEMBER_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Some(user),
                &None::<Uuid>,
                &Uuid::new_v4(),
                &project.id,
                &workspace.id,
                &member,
                &role,
                &JsonText::new(MEMBER_PROPS),
                &JsonText::new(MEMBER_PROPS),
                &JsonText::new(MEMBER_PREFERENCES),
                &65535.0_f64,
            ],
        )
        .await;
    map_write_error(result)
}

/// `State.objects.bulk_create(DEFAULT_STATES)` (`project/base.py:282-296`).
///
/// `bulk_create` never calls `State.save()`, so H19 (`slugify`, `MAX(sequence)`)
/// does not run: `slug` stays `''`, the sequences are the constants, and
/// `is_triage` stays false.
async fn insert_default_states(
    client: &Client,
    workspace: &Workspace,
    project: &Project,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let mut sql = String::from(
        "INSERT INTO states (created_at, updated_at, created_by_id, updated_by_id, \
         deleted_at, id, project_id, workspace_id, name, description, color, slug, \
         sequence, \"group\", is_triage, \"default\") VALUES ",
    );
    let ids: Vec<Uuid> = (0..DEFAULT_STATES.len()).map(|_| Uuid::new_v4()).collect();
    let mut params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
        Vec::with_capacity(DEFAULT_STATES.len() * 11);
    for (index, state) in DEFAULT_STATES.iter().enumerate() {
        if index != 0 {
            sql.push_str(", ");
        }
        let base = index * 11;
        sql.push_str(&format!(
            "(${}, ${}, ${}, NULL, NULL, ${}, ${}, ${}, ${}, '', ${}, '', ${}, ${}, FALSE, ${})",
            base + 1,
            base + 2,
            base + 3,
            base + 4,
            base + 5,
            base + 6,
            base + 7,
            base + 8,
            base + 9,
            base + 10,
            base + 11
        ));
        params.push(&now);
        params.push(&now);
        params.push(&user);
        params.push(&ids[index]);
        params.push(&project.id);
        params.push(&workspace.id);
        params.push(&state.name);
        params.push(&state.color);
        params.push(&state.sequence);
        params.push(&state.group);
        params.push(&state.default);
    }
    let statement = client.prepare(&sql).await.map_err(ViewError::from)?;
    let result = client.execute(&statement, &params).await;
    map_write_error(result)
}

async fn get_project(client: &Client, project_id: Uuid, slug: &str) -> Result<Project, ViewError> {
    let statement = client
        .prepare_cached(queries::PROJECT_BY_PK.sql)
        .await
        .map_err(ViewError::from)?;
    match client
        .query_opt(&statement, &[&project_id, &slug])
        .await
        .map_err(ViewError::from)?
    {
        Some(row) => Ok(Project::from_row(&row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the project row: {error}")))?),
        None => Err(ViewError::ObjectDoesNotExist),
    }
}

async fn is_workspace_admin(client: &Client, slug: &str, user: Uuid) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(queries::WORKSPACE_ADMIN_EXISTS.sql, &[&user, &slug])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

async fn is_project_admin(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user: Uuid,
) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(
            queries::PROJECT_ADMIN_EXISTS.sql,
            &[&user, &slug, &project_id],
        )
        .await
        .map_err(ViewError::from)?
        .is_some())
}

async fn default_intake_exists(client: &Client, project_id: Uuid) -> Result<bool, ViewError> {
    Ok(client
        .query_opt(queries::DEFAULT_INTAKE_EXISTS.sql, &[&project_id])
        .await
        .map_err(ViewError::from)?
        .is_some())
}

async fn insert_intake(
    client: &Client,
    workspace: &Workspace,
    project: &Project,
    user: Uuid,
    now: DateTime<Utc>,
) -> Result<(), ViewError> {
    let statement = client
        .prepare_cached(queries::INTAKE_INSERT.sql)
        .await
        .map_err(ViewError::from)?;
    let result = client
        .execute(
            &statement,
            &[
                &now,
                &now,
                &Some(user),
                &None::<Uuid>,
                &Uuid::new_v4(),
                &project.id,
                &workspace.id,
                &format!("{} Intake", project.name),
            ],
        )
        .await;
    map_write_error(result)
}

// ---------------------------------------------------------------------------
// `validate()`'s HTML branch
// ---------------------------------------------------------------------------

/// `validate_html_content(str(data["description_html"]))`
/// (`serializers/project.py:104-112`, `plane/utils/content_validator.py`).
///
/// `str()` of a JSON value: a string stays itself, everything else becomes its
/// Python `str()`; an empty one is falsy and skipped. The sanitised string
/// replaces the value (even when the input was not a string), and a failure is
/// `ValidationError({"error": "html content is not valid"})` — rendered as
/// `{"error": ["html content is not valid"]}`.
fn sanitize_html(
    value: &Option<JsonText>,
) -> Result<Result<Option<JsonText>, ViewResponse>, ViewError> {
    let Some(value) = value else {
        return Ok(Ok(None));
    };
    let parsed = plane_serde::pyjson::loads(value.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(error.message().to_string()))?;
    if parsed.is_null() {
        return Ok(Ok(Some(value.clone())));
    }
    let text = python_str(&parsed);
    if text.is_empty() {
        return Ok(Ok(Some(value.clone())));
    }
    if text.len() > 10 * 1024 * 1024 {
        return Ok(Err(write::field_string_error(
            "error",
            "html content is not valid",
        )));
    }
    let sanitized = crate::write::html::sanitize(&text);
    let stored = JsonText::from_json(&Json::Str(PyStr::from(sanitized.as_str())))
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(Ok(Some(stored)))
}

/// Python `str(value)` for a `json.loads` value (`str()` on a string returns
/// the string; composites use `repr`, which `nh3` then treats as text).
fn python_str(value: &Json) -> String {
    match value {
        Json::Null => "None".to_owned(),
        Json::Bool(value) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Int(value) => value.to_string(),
        Json::Float(value) => plane_query::pyfloat::repr(*value),
        Json::Str(value) => value.as_str().map(str::to_owned).unwrap_or_default(),
        Json::List(_) | Json::Dict(_) => python_repr(value),
    }
}

/// Python `repr(value)` for the composite branch of [`python_str`].
fn python_repr(value: &Json) -> String {
    match value {
        Json::List(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Dict(object) => {
            let parts: Vec<String> = object
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        repr_str(key.as_str().unwrap_or_default()),
                        python_repr(value)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        other => python_str(other),
    }
}

/// Python `repr(str)` with single quotes (the common `str(dict)` case).
fn repr_str(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

/// Python truthiness of an injected `intake_view` value (`if intake_view:`).
fn py_truthy(raw: &Raw<'_>) -> bool {
    match raw {
        Raw::Text(text) => !text.is_empty(),
        Raw::Json(value) => match value {
            Json::Null => false,
            Json::Bool(value) => *value,
            Json::Int(value) => value.as_i64().is_some_and(|value| value != 0),
            Json::Float(value) => *value != 0.0,
            Json::Str(value) => value.as_str().is_some_and(|text| !text.is_empty()),
            Json::List(items) => !items.is_empty(),
            Json::Dict(object) => !object.is_empty(),
        },
    }
}

// ---------------------------------------------------------------------------
// Activity / webhook tasks
// ---------------------------------------------------------------------------

/// `base_host(request=request, is_app=True)` (`plane/utils/host.py:17-57`).
fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `model_activity.delay(...)` (`project/base.py:300-309`, `:365-374`).
async fn publish_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    project_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message =
        plane_jobs::celery::tasks::model_activity(plane_jobs::celery::tasks::ModelActivityCall {
            model_name: "project".to_owned(),
            model_id: project_id.to_string(),
            requested_data: write::request_data_arg(ctx)?,
            current_instance,
            actor_id: user,
            slug: slug.to_owned(),
            origin: base_host(ctx)?,
        });
    crate::common::jobs::dispatch_task(
        ctx,
        task,
        &message,
        plane_jobs::queue::Job::from_message(&message),
    )
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// `current_instance`: json.dumps(ProjectSerializer(project).data,
//                                 cls=DjangoJSONEncoder)
// ---------------------------------------------------------------------------

/// Build the activity message's `current_instance` string.
///
/// The field order is `ProjectSerializer.data`'s: `id`, `workspace_detail`,
/// `inbox_view`, the model fields in `Project._meta.fields` order (minus the
/// pk and the declared names), then the forward relations.
async fn current_instance_json(
    client: &Client,
    project: &Project,
    workspace: &Workspace,
) -> Result<String, ViewError> {
    let logo_url = workspace_logo_url(client, workspace).await?;
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    out.push(b'{');
    let mut first = true;

    push_string(&mut out, &mut first, "id", &project.id.to_string())?;

    // `workspace_detail = WorkspaceLiteSerializer(workspace).data`:
    // `name`, `slug`, `id`, `logo_url` (`serializers/workspace.py:87-91`).
    push_key(&mut out, &mut first, "workspace_detail");
    out.push(b'{');
    let mut inner = true;
    push_string(&mut out, &mut inner, "name", &workspace.name)?;
    push_string(&mut out, &mut inner, "slug", &workspace.slug)?;
    push_string(&mut out, &mut inner, "id", &workspace.id.to_string())?;
    push_value(&mut out, &mut inner, "logo_url", &logo_url)?;
    out.push(b'}');

    push_value(&mut out, &mut first, "inbox_view", &project.intake_view)?;
    push_datetime(&mut out, &mut first, "created_at", project.created_at)?;
    push_datetime(&mut out, &mut first, "updated_at", project.updated_at)?;
    push_datetime_opt(&mut out, &mut first, "deleted_at", project.deleted_at)?;
    push_string(&mut out, &mut first, "name", &project.name)?;
    push_string(&mut out, &mut first, "description", &project.description)?;
    push_json(
        &mut out,
        &mut first,
        "description_text",
        project.description_text.as_ref(),
    )?;
    push_json(
        &mut out,
        &mut first,
        "description_html",
        project.description_html.as_ref(),
    )?;
    push_value(&mut out, &mut first, "network", &project.network)?;
    push_string(&mut out, &mut first, "identifier", &project.identifier)?;
    push_value(&mut out, &mut first, "emoji", &project.emoji)?;
    push_json(
        &mut out,
        &mut first,
        "icon_prop",
        project.icon_prop.as_ref(),
    )?;
    push_value(&mut out, &mut first, "module_view", &project.module_view)?;
    push_value(&mut out, &mut first, "cycle_view", &project.cycle_view)?;
    push_value(
        &mut out,
        &mut first,
        "issue_views_view",
        &project.issue_views_view,
    )?;
    push_value(&mut out, &mut first, "page_view", &project.page_view)?;
    push_value(&mut out, &mut first, "intake_view", &project.intake_view)?;
    push_value(
        &mut out,
        &mut first,
        "is_time_tracking_enabled",
        &project.is_time_tracking_enabled,
    )?;
    push_value(
        &mut out,
        &mut first,
        "is_issue_type_enabled",
        &project.is_issue_type_enabled,
    )?;
    push_value(
        &mut out,
        &mut first,
        "guest_view_all_features",
        &project.guest_view_all_features,
    )?;
    push_value(&mut out, &mut first, "cover_image", &project.cover_image)?;
    push_value(&mut out, &mut first, "archive_in", &project.archive_in)?;
    push_value(&mut out, &mut first, "close_in", &project.close_in)?;
    push_json(
        &mut out,
        &mut first,
        "logo_props",
        Some(&project.logo_props),
    )?;
    push_datetime_opt(&mut out, &mut first, "archived_at", project.archived_at)?;
    push_string(&mut out, &mut first, "timezone", &project.timezone)?;
    push_value(
        &mut out,
        &mut first,
        "external_source",
        &project.external_source,
    )?;
    push_value(&mut out, &mut first, "external_id", &project.external_id)?;
    push_id(&mut out, &mut first, "created_by", project.created_by_id)?;
    push_id(&mut out, &mut first, "updated_by", project.updated_by_id)?;
    push_id(
        &mut out,
        &mut first,
        "workspace",
        Some(project.workspace_id),
    )?;
    push_id(
        &mut out,
        &mut first,
        "default_assignee",
        project.default_assignee_id,
    )?;
    push_id(
        &mut out,
        &mut first,
        "project_lead",
        project.project_lead_id,
    )?;
    push_id(
        &mut out,
        &mut first,
        "cover_image_asset",
        project.cover_image_asset_id,
    )?;
    push_id(&mut out, &mut first, "estimate", project.estimate_id)?;
    push_id(
        &mut out,
        &mut first,
        "default_state",
        project.default_state_id,
    )?;

    out.push(b'}');
    String::from_utf8(out)
        .map_err(|error| ViewError::Unknown(format!("json.dumps produced non-UTF-8: {error}")))
}

/// `Workspace.logo_url` (`plane/db/models/workspace.py:145-154`): the related
/// `FileAsset.asset_url` when there is one, else the `logo` text, else `None`.
async fn workspace_logo_url(
    client: &Client,
    workspace: &Workspace,
) -> Result<Option<String>, ViewError> {
    let asset = match workspace.logo_asset_id {
        Some(id) => {
            let statement = client
                .prepare_cached(queries::WORKSPACE_LOGO_ASSET.sql)
                .await
                .map_err(ViewError::from)?;
            match client
                .query_opt(&statement, &[&id])
                .await
                .map_err(ViewError::from)?
            {
                Some(row) => AssetRef::from_row(&row, 0)
                    .map_err(|error| ViewError::Unknown(error.to_string()))?,
                None => AssetRef::default(),
            }
        }
        None => AssetRef::default(),
    };
    asset_url::workspace_logo_url(workspace.logo.as_deref(), &asset)
}

fn push_key(out: &mut Vec<u8>, first: &mut bool, key: &str) {
    if *first {
        *first = false;
    } else {
        out.extend_from_slice(b", ");
    }
    out.push(b'"');
    out.extend_from_slice(key.as_bytes());
    out.extend_from_slice(b"\": ");
}

fn push_value<T>(out: &mut Vec<u8>, first: &mut bool, key: &str, value: &T) -> Result<(), ViewError>
where
    T: serde::Serialize + ?Sized,
{
    push_key(out, first, key);
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    out.extend_from_slice(&bytes);
    Ok(())
}

fn push_string(
    out: &mut Vec<u8>,
    first: &mut bool,
    key: &str,
    value: &str,
) -> Result<(), ViewError> {
    push_value(out, first, key, value)
}

fn push_id(
    out: &mut Vec<u8>,
    first: &mut bool,
    key: &str,
    value: Option<Uuid>,
) -> Result<(), ViewError> {
    match value {
        Some(id) => push_value(out, first, key, &id.to_string()),
        None => {
            push_key(out, first, key);
            out.extend_from_slice(b"null");
            Ok(())
        }
    }
}

fn push_datetime(
    out: &mut Vec<u8>,
    first: &mut bool,
    key: &str,
    value: DateTime<Utc>,
) -> Result<(), ViewError> {
    // `ProjectSerializer(project).data` is a *serialized* dict: DRF's
    // `DateTimeField.to_representation` already turned the datetime into an
    // ISO-8601 **string** (microseconds kept, `+00:00` -> `Z`), so
    // `json.dumps` never reaches `DjangoJSONEncoder.default` for it. Using the
    // Django encoder's millisecond form here shortened the repr by three
    // characters and shifted the `saferepr` truncation point.
    push_value(out, first, key, &plane_serde::values::DrfDateTime(value))
}

fn push_datetime_opt(
    out: &mut Vec<u8>,
    first: &mut bool,
    key: &str,
    value: Option<DateTime<Utc>>,
) -> Result<(), ViewError> {
    match value {
        Some(value) => push_datetime(out, first, key, value),
        None => {
            push_key(out, first, key);
            out.extend_from_slice(b"null");
            Ok(())
        }
    }
}

/// A `JSONField` value: `json.dumps` of the object `json.loads` produced.
fn push_json(
    out: &mut Vec<u8>,
    first: &mut bool,
    key: &str,
    value: Option<&JsonText>,
) -> Result<(), ViewError> {
    push_key(out, first, key);
    match value {
        Some(value) => {
            plane_serde::transcode_json(plane_serde::Dialect::Django, value.as_str(), out)
                .map_err(|error| ViewError::Unknown(error.to_string()))?
        }
        None => out.extend_from_slice(b"null"),
    }
    Ok(())
}

/// `PUT` (`rest_framework.mixins.UpdateModelMixin.update`) is refused by
/// `BaseViewSet`'s guard (`routes.json` `viewset_guard.refused_methods`): the
/// runtime answers 405 before dispatch, so this handler is unreachable.
pub fn guard_only<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        Err(ViewError::Unknown(format!(
            "{} reached a handler its BaseViewSet guard always refuses",
            ctx.view
        )))
    })
}
