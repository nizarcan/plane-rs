// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `fields=` / `expand=` rules, isolated from any generated serializer.
//!
//! `tests/golden.rs` proves the same behaviour against Django for real
//! serializers; these cases pin the corner cases a single golden would hide
//! (dict-item masks with several keys, `expand` order and replacement, the
//! error for a forgotten expansion, the v1 base's no-injection rule).

use chrono_tz::Tz;
use plane_serializers::{
    DrfError, DrfSerialize, DrfValue, DrfWriter, Expansion, Expansions, FieldEntry, FilterMode,
    RenderCtx,
};

/// `plane.app.serializers.base.DynamicBaseSerializer` behaviour.
#[derive(Clone, Debug)]
struct Dynamic {
    id: i64,
    name: &'static str,
    state: i64,
}

const DYNAMIC_FIELDS: &[&str] = &["id", "name", "state"];

impl DrfSerialize for Dynamic {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", FilterMode::OutputOnly) {
            obj.field_dyn("id", ctx, &self.id)?;
        }
        if ctx.emits("name", FilterMode::OutputOnly) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        if ctx.emits("state", FilterMode::OutputOnly) {
            obj.field_dyn("state", ctx, &self.state)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, DYNAMIC_FIELDS, FilterMode::OutputOnly)?;
        obj.end();
        Ok(())
    }
}

/// `plane.api.serializers.base.BaseSerializer` behaviour.
#[derive(Clone, Debug)]
struct V1 {
    id: i64,
    name: &'static str,
}

const V1_FIELDS: &[&str] = &["id", "name"];

impl DrfSerialize for V1 {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        if ctx.emits("id", FilterMode::RemoveFields) {
            obj.field_dyn("id", ctx, &self.id)?;
        }
        if ctx.emits("name", FilterMode::RemoveFields) {
            obj.field_dyn("name", ctx, &self.name)?;
        }
        ctx.expansions()
            .write_injected(&mut obj, ctx, V1_FIELDS, FilterMode::RemoveFields)?;
        obj.end();
        Ok(())
    }
}

const UTC: Tz = chrono_tz::UTC;

fn dynamic() -> Dynamic {
    Dynamic {
        id: 1,
        name: "n",
        state: 7,
    }
}

fn render<T: DrfSerialize>(value: &T, ctx: &RenderCtx<'_>) -> String {
    String::from_utf8(plane_serializers::render(value, ctx).expect("render")).expect("utf-8")
}

fn names<'a>(names: &'a [&'a str]) -> Vec<FieldEntry<'a>> {
    names.iter().copied().map(FieldEntry::Name).collect()
}

#[test]
fn no_arguments_renders_every_field_in_declaration_order() {
    let ctx = RenderCtx::new(&UTC, None, &[]);
    assert_eq!(
        render(&dynamic(), &ctx),
        "{\"id\":1,\"name\":\"n\",\"state\":7}"
    );
}

#[test]
fn fields_keeps_declaration_order_not_request_order() {
    let entries = names(&["state", "id"]);
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    assert_eq!(render(&dynamic(), &ctx), "{\"id\":1,\"state\":7}");
}

#[test]
fn a_fields_dict_item_allows_every_key_and_ignores_the_value() {
    let nested: Vec<FieldEntry<'_>> = Vec::new();
    let entries = vec![FieldEntry::Nested {
        names: &["state", "name"],
        fields: &nested,
    }];
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    assert_eq!(render(&dynamic(), &ctx), "{\"name\":\"n\",\"state\":7}");
}

#[test]
fn an_empty_fields_list_means_nothing_for_the_dynamic_base() {
    let entries: Vec<FieldEntry<'_>> = Vec::new();
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    assert_eq!(render(&dynamic(), &ctx), "{}");
}

#[test]
fn an_empty_fields_list_does_not_filter_for_the_v1_base() {
    let entries: Vec<FieldEntry<'_>> = Vec::new();
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    let value = V1 { id: 2, name: "v" };
    assert_eq!(render(&value, &ctx), "{\"id\":2,\"name\":\"v\"}");
}

#[test]
fn unknown_fields_names_are_ignored() {
    let entries = names(&["id", "not_a_field"]);
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    assert_eq!(render(&dynamic(), &ctx), "{\"id\":1}");
}

#[test]
fn expand_appends_injected_fields_in_request_order() {
    let user = V1 { id: 8, name: "u" };
    let project = V1 { id: 9, name: "p" };
    let values = [
        Expansion {
            name: "created_by",
            value: DrfValue::nested(&user),
        },
        Expansion {
            name: "project",
            value: DrfValue::nested(&project),
        },
    ];
    let requested = ["created_by", "project"];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    assert_eq!(
        render(&dynamic(), &ctx),
        "{\"id\":1,\"name\":\"n\",\"state\":7,\"created_by\":{\"id\":8,\"name\":\"u\"},\
         \"project\":{\"id\":9,\"name\":\"p\"}}"
    );
}

#[test]
fn expand_replaces_a_base_field_in_place_and_null_keeps_the_key() {
    let nested = V1 { id: 7, name: "s" };
    let values = [Expansion {
        name: "state",
        value: DrfValue::nested(&nested),
    }];
    let requested = ["state"];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    assert_eq!(
        render(&dynamic(), &ctx),
        "{\"id\":1,\"name\":\"n\",\"state\":{\"id\":7,\"name\":\"s\"}}"
    );

    let values = [Expansion {
        name: "state",
        value: DrfValue::Null,
    }];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    assert_eq!(
        render(&dynamic(), &ctx),
        "{\"id\":1,\"name\":\"n\",\"state\":null}"
    );
}

#[test]
fn issue_attachments_is_always_last() {
    let attachments = vec![DrfValue::Int(1)];
    let values = [Expansion {
        name: "issue_attachments",
        value: DrfValue::Array(attachments),
    }];
    let requested = ["issue_attachments"];
    let entries = names(&["id"]);
    let ctx = RenderCtx::new(&UTC, Some(&entries), &requested)
        .with_expansions(Expansions::new(&requested, &values));
    assert_eq!(
        render(&dynamic(), &ctx),
        "{\"id\":1,\"issue_attachments\":[1]}"
    );
}

#[test]
fn issue_attachments_without_expand_does_not_add_a_key() {
    // `fields=["issue_attachments"]` alone: the dynamic base only runs the
    // attachments block inside `if self.expand` (`base.py:142-188`).
    let entries = names(&["id", "issue_attachments"]);
    let ctx = RenderCtx::new(&UTC, Some(&entries), &[]);
    assert_eq!(render(&dynamic(), &ctx), "{\"id\":1}");
}

#[test]
fn a_mapper_name_without_a_value_is_an_error() {
    let requested = ["state"];
    let values: [Expansion<'_>; 0] = [];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    let error = plane_serializers::render(&dynamic(), &ctx).expect_err("missing expansion");
    assert!(matches!(error, DrfError::MissingExpansion(name) if name == "state"));
}

#[test]
fn the_v1_base_never_injects() {
    let requested = ["state"];
    let nested = V1 { id: 7, name: "s" };
    let values = [Expansion {
        name: "state",
        value: DrfValue::nested(&nested),
    }];
    // `state` is not a V1 field, so the request is ignored rather than
    // injected; `fields=` decides what the response contains.
    let entries = names(&["id"]);
    let ctx = RenderCtx::new(&UTC, Some(&entries), &requested)
        .with_expansions(Expansions::new(&requested, &values));
    let value = V1 { id: 2, name: "v" };
    assert_eq!(render(&value, &ctx), "{\"id\":2}");
}

#[test]
fn a_requested_v1_base_field_must_be_resolved() {
    // `name` is a base field of the v1 serializer and `expand=` replaces it
    // with `name_id` (an attribute that does not exist, so `null`), not with
    // the field's own value.
    let requested = ["name"];
    let values: [Expansion<'_>; 0] = [];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    let value = V1 { id: 2, name: "v" };
    let error = plane_serializers::render(&value, &ctx).expect_err("missing expansion");
    assert!(matches!(error, DrfError::MissingExpansion(name) if name == "name"));
}

#[test]
fn requested_base_field_must_be_resolved() {
    // `state` is a field: DRF replaces its value, so a handler that forgets the
    // relation must not silently render the default.
    let requested = ["state"];
    let values: [Expansion<'_>; 0] = [];
    let ctx = RenderCtx::new(&UTC, None, &requested)
        .with_expansions(Expansions::new(&requested, &values));
    let error = plane_serializers::render(&dynamic(), &ctx).expect_err("missing expansion");
    assert!(matches!(error, DrfError::MissingExpansion(name) if name == "state"));
}

/// `ProjectMemberSerializer` with `fields=`/`expand=`, byte-compared with what
/// DRF 3.18.3 answers for
///
/// ```python
/// ProjectMember(id="1111…", member_id="2222…", role=15)
/// ProjectMemberSerializer(instance, fields=…, expand=…)
/// JSONRenderer().render(serializer.data)
/// ```
///
/// Recorded from `apps/api` on 2026-11-17. The three shapes the v1 base has
/// that the app's does not:
///
/// * `expand=member` — `member` is a concrete FK, so `getattr(instance,
///   "member_id")` is the UUID the unexpanded field already renders;
/// * `expand=role` — a `ChoiceField` has no `role_id`, so the key becomes
///   `null`;
/// * `expand=id` — the model's own primary key has no `id_id` either.
#[test]
fn the_v1_id_fallback_matches_drf_on_project_member() {
    use plane_serializers::generated::api_member::ProjectMemberSerializer;
    use plane_serializers::v1::{id_fallback, is_expansion_name};
    use uuid::Uuid;

    let id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").expect("id");
    let member = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("member");
    let value = ProjectMemberSerializer {
        id,
        member: Some(member),
        role: 15,
    };

    // `id_fallback` values exactly as the handler would supply them: `member`
    // is a FK (the pk), `role` and `id` are not.
    type Case = (
        Option<&'static [&'static str]>,
        &'static [&'static str],
        &'static str,
    );
    let cases: &[Case] = &[
        (
            None,
            &[],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
        (
            Some(&[]),
            &[],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
        (
            Some(&["id"]),
            &[],
            r#"{"id":"11111111-1111-4111-8111-111111111111"}"#,
        ),
        // expand=member: a FK, so the id fallback is the same UUID.
        (
            None,
            &["member"],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
        // expand=role: no `role_id`, so the key is null.
        (
            None,
            &["role"],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":null}"#,
        ),
        // expand=id: no `id_id`.
        (
            None,
            &["id"],
            r#"{"id":null,"member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
        // Both at once: member keeps its value (not `member_id`), role nulls.
        (
            None,
            &["member", "role"],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":null}"#,
        ),
        // `fields=` removed `member`, so `expand=member` finds nothing to
        // replace (`base.py:85`).
        (
            Some(&["id"]),
            &["member"],
            r#"{"id":"11111111-1111-4111-8111-111111111111"}"#,
        ),
        // A mapper name that is not a field of this serializer is ignored.
        (
            None,
            &["user"],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
        (
            None,
            &["workspace"],
            r#"{"id":"11111111-1111-4111-8111-111111111111","member":"22222222-2222-4222-8222-222222222222","role":15}"#,
        ),
    ];

    for (fields, expand, expected) in cases {
        let entries = fields.map(|names| {
            names
                .iter()
                .copied()
                .map(FieldEntry::Name)
                .collect::<Vec<FieldEntry<'_>>>()
        });
        let id_values: Vec<Expansion<'_>> = expand
            .iter()
            .filter(|name| !is_expansion_name(name))
            .map(|name| Expansion {
                name,
                value: if *name == "member" {
                    id_fallback(Some(member))
                } else {
                    id_fallback(None)
                },
            })
            .collect();
        let ctx = RenderCtx::new(&UTC, entries.as_deref(), expand)
            .with_expansions(Expansions::new(expand, &id_values));
        assert_eq!(
            render(&value, &ctx),
            *expected,
            "fields={fields:?} expand={expand:?}"
        );
    }
}
