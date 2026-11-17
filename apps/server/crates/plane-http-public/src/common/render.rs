// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `fields=`/`expand=` for `/api/v1/`: the shared renderer plus v1's map.
//!
//! The rendering itself is the app surface's: [`RequestRender`] reads
//! `?fields=`/`?expand=` off the request and resolves the active timezone, and
//! the generated serializers carry their own `fields=` mode
//! (`FilterMode::RemoveFields` for every `plane.api.serializers.base
//! .BaseSerializer` subclass). Two things are v1-only:
//!
//! * the mapper is `plane_serializers::v1::V1_EXPANSION_MAPPER` (14 names),
//!   not the app's 22;
//! * a name that is not in the mapper is replaced by
//!   `getattr(instance, f"{name}_id", None)` — [`plane_serializers::v1::id_fallback`].
//!
//! [`expansions`] is the one call that turns a request into the values a render
//! needs: a mapper name comes from what the handler resolved against the
//! instance, every other name from the `_id` lookup. A handler that forgets a
//! name cannot silently render the unexpanded value — `Expansions::write_injected`
//! refuses an emitted base field without a resolution
//! (`plane/api/serializers/base.py:139`).
//!
//! ```ignore
//! use plane_http_public::common::render::{self, expansions};
//!
//! let requested = ctx.expand().unwrap_or_default();
//! let resolved = [
//!     Expansion { name: "state", value: DrfValue::nested(&state_lite) },
//! ];
//! // `member` is a FK; `role` and everything else has no `<name>_id`.
//! let values = expansions(&requested, &resolved, |name| match name {
//!     "member" => Some(member_id),
//!     _ => None,
//! });
//! let ctx = RequestRender::from_ctx(ctx)
//!     .render_ctx()
//!     .with_expansions(Expansions::new(&requested, &values));
//! let body = plane_serializers::render(&serializer, &ctx)?;
//! ```
//!
//! `fields=` and `expand=` are only honoured by handlers that pass them
//! explicitly: `plane/api/views/issue.py:1103-1110` does not, `state.py:217-221`
//! and `project.py:206,526` do. Match the Python per route.

pub use plane_http_app::common::{RequestRender, render, render_list};
use plane_serializers::v1;
use plane_serializers::{DrfValue, Expansion};

/// Every `expand=` value a v1 render needs, in request order.
///
/// `resolved` is what the handler read off the instance for the **mapper**
/// names it supports (a `models.Manager` → `DrfValue::Array`, a model →
/// `DrfValue::Nested`, a null relation → `DrfValue::Null`). A requested name
/// that is not in the mapper is `getattr(instance, "<name>_id", None)`, which
/// `id_of` answers: `Some(pk)` for a concrete relation, `None` otherwise.
///
/// A mapper name the handler did not resolve also falls back to `id_of`, which
/// is what Django does when the attribute is not a relation it can serialize
/// (`plane/api/serializers/base.py:121-136` leaves the value the serializer
/// already produced; the `_id` attribute does not exist, so `:139`'s branch is
/// not what runs — `None` is the closest faithful answer and is exact for every
/// name that is not a relation).
#[must_use]
pub fn expansions<'a, F>(
    requested: &'a [&'a str],
    resolved: &[Expansion<'a>],
    id_of: F,
) -> Vec<Expansion<'a>>
where
    F: Fn(&str) -> Option<uuid::Uuid>,
{
    let mut values = Vec::with_capacity(requested.len());
    for &name in requested {
        let value = match resolved.iter().find(|entry| entry.name == name) {
            Some(entry) => entry.value.clone(),
            None => v1::id_fallback(id_of(name)),
        };
        values.push(Expansion { name, value });
    }
    values
}

/// Whether `expand=name` resolves through v1's mapper rather than through the
/// `_id` fallback. Re-exported so a handler does not depend on
/// `plane-serializers` directly for one predicate.
#[must_use]
pub fn is_mapper_name(name: &str) -> bool {
    v1::is_expansion_name(name)
}

/// The `_id` fallback for one name, re-exported for the same reason.
#[must_use]
pub fn id_fallback(id: Option<uuid::Uuid>) -> DrfValue<'static> {
    v1::id_fallback(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serializers::{
        DrfError, DrfSerialize, DrfWriter, Expansions, FieldEntry, FilterMode, RenderCtx,
    };
    use uuid::Uuid;

    /// A v1 serializer with one relation field and one scalar field, honouring
    /// `fields=` exactly as the generated modules do.
    #[derive(Debug)]
    struct Probe {
        member: Option<Uuid>,
        role: i64,
    }

    const FIELDS: &[&str] = &["member", "role"];

    impl DrfSerialize for Probe {
        fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
            let mut obj = w.begin_object();
            if ctx.emits("member", FilterMode::RemoveFields) {
                obj.field_dyn("member", ctx, &self.member.map(plane_serializers::DrfUuid))?;
            }
            if ctx.emits("role", FilterMode::RemoveFields) {
                obj.field_dyn("role", ctx, &self.role)?;
            }
            ctx.expansions()
                .write_injected(&mut obj, ctx, FIELDS, FilterMode::RemoveFields)?;
            obj.end();
            Ok(())
        }
    }

    const UTC: chrono_tz::Tz = chrono_tz::UTC;

    fn render(probe: &Probe, requested: &[&str]) -> String {
        let values = expansions(requested, &[], |name| match name {
            "member" => probe.member,
            _ => None,
        });
        let ctx = RenderCtx::new(&UTC, None, requested)
            .with_expansions(Expansions::new(requested, &values));
        String::from_utf8(plane_serializers::render(probe, &ctx).expect("render")).expect("utf-8")
    }

    /// The three shapes the plan calls out: a FK is its own pk, a scalar is
    /// `null`, and a mapper name that is not a field adds nothing.
    #[test]
    fn the_id_fallback_is_exact() {
        let member = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid");
        let probe = Probe {
            member: Some(member),
            role: 15,
        };
        assert_eq!(
            render(&probe, &[]),
            r#"{"member":"22222222-2222-4222-8222-222222222222","role":15}"#
        );
        // expand=member: a relation, so `<name>_id` is the same UUID.
        assert_eq!(
            render(&probe, &["member"]),
            r#"{"member":"22222222-2222-4222-8222-222222222222","role":15}"#
        );
        // expand=role: no `role_id`, so the key becomes null.
        assert_eq!(
            render(&probe, &["role"]),
            r#"{"member":"22222222-2222-4222-8222-222222222222","role":null}"#
        );
        // expand=user: a mapper name, but not a field of this serializer, so
        // the request changes nothing (`plane/api/serializers/base.py:85`).
        assert_eq!(
            render(&probe, &["user"]),
            r#"{"member":"22222222-2222-4222-8222-222222222222","role":15}"#
        );
    }

    /// A mapper name the handler *did* resolve wins over the `_id` fallback.
    #[test]
    fn a_resolved_mapper_name_replaces_the_value() {
        let member = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid");
        let probe = Probe {
            member: Some(member),
            role: 15,
        };
        #[derive(Debug)]
        struct Lite;
        impl DrfSerialize for Lite {
            fn drf_write(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
                let mut obj = w.begin_object();
                obj.field("id", &1)?;
                obj.end();
                Ok(())
            }
        }
        let lite = Lite;
        let resolved = [Expansion {
            name: "member",
            value: DrfValue::nested(&lite),
        }];
        let requested = ["member"];
        let values = expansions(&requested, &resolved, |_| None);
        let ctx = RenderCtx::new(&UTC, None, &requested)
            .with_expansions(Expansions::new(&requested, &values));
        let body = plane_serializers::render(&probe, &ctx).expect("render");
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            r#"{"member":{"id":1},"role":15}"#
        );
    }

    /// `fields=` removes a field before `expand=` runs, so the expansion finds
    /// nothing to replace even when a value was resolved.
    #[test]
    fn a_removed_field_is_not_expanded() {
        let member = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid");
        let probe = Probe {
            member: Some(member),
            role: 15,
        };
        let entries = [FieldEntry::Name("role")];
        let requested = ["member"];
        let values = expansions(&requested, &[], |name| match name {
            "member" => probe.member,
            _ => None,
        });
        let ctx = RenderCtx::new(&UTC, Some(&entries), &requested)
            .with_expansions(Expansions::new(&requested, &values));
        let body = plane_serializers::render(&probe, &ctx).expect("render");
        assert_eq!(String::from_utf8(body).expect("utf-8"), r#"{"role":15}"#);
    }

    #[test]
    fn the_mapper_predicate_is_the_v1_map() {
        assert!(is_mapper_name("estimate_point"));
        assert!(!is_mapper_name("assignees"));
        assert!(!is_mapper_name("role"));
    }
}
