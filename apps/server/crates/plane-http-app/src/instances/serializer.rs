// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.license.api.serializers.configuration.InstanceConfigurationSerializer`
//! — the one serializer on the instance surface with a hand-written
//! `to_representation` (`PUBLIC_INSTANCES.md` §2e).
//!
//! ```python
//! class InstanceConfigurationSerializer(BaseSerializer):
//!     class Meta:
//!         model = InstanceConfiguration
//!         fields = "__all__"
//!
//!     def to_representation(self, instance):
//!         data = super().to_representation(instance)
//!         # Decrypt secrets value
//!         if instance.is_encrypted and instance.value is not None:
//!             data["value"] = decrypt_data(instance.value)
//!         return data
//! ```
//!
//! (`apps/api/plane/license/api/serializers/configuration.py:8-20`.)
//!
//! Two rules make this "manual" in `SERIALIZERS.md` rather than generated:
//!
//! * `value` is decrypted **only** when the row is marked encrypted *and* the
//!   column is not `NULL`; an encrypted `NULL` stays `null`, and a plaintext
//!   row renders its stored text unchanged;
//! * a decryption failure is `""`, not an error — `decrypt_data` swallows every
//!   exception (`plane/license/utils/encryption.py:31-44`), so a deployment
//!   whose `SECRET_KEY` rotated answers `"value": ""`.
//!
//! The field order is the model's, exactly as `fields = "__all__"` builds it:
//! `id, created_at, updated_at, deleted_at, key, value, category,
//! is_encrypted, created_by, updated_by` (`SERIALIZERS.md:4239-4256`). The base
//! class is a plain `ModelSerializer`, so `?fields=` and `?expand=` do not
//! apply ([`plane_serializers::FilterMode::None`]) even though the view exposes
//! the properties (`plane/license/api/views/base.py:98-106`).
//!
//! ## §2d — what `PATCH` writes
//!
//! `InstanceConfigurationEndpoint.patch` (`configuration.py:41-58`) turns the
//! body into a value per row and then calls
//! `bulk_update(bulk_configurations, ["value"], batch_size=100)`:
//!
//! ```python
//! raw_value = request.data.get(configuration.key, configuration.value)
//! value = "" if raw_value is None else str(raw_value).strip()
//! ```
//!
//! so `str()` semantics (Python `repr` for everything but a string,
//! `write::fields::python_str`) and `str.strip()`'s whitespace classes apply to
//! a value the client may have sent as any JSON type. [`body_value`] and
//! [`stored_value`] are those two branches. The write itself — only `value`,
//! never `updated_at` or an audit column, never an insert — is
//! [`super::bulk_update_values`].

use plane_db::models::license::instance::InstanceConfiguration;
use plane_serde::pyjson::Value as Json;
use plane_serializers::{
    DrfError, DrfSerialize, DrfUuid, DrfWriter, FilterMode, RenderCtx, drf_datetime,
    drf_datetime_opt,
};

use crate::write::fields::{Raw, python_str};

/// `InstanceConfigurationSerializer(row)`, borrowing the row and the process's
/// `SECRET_KEY`.
#[derive(Debug)]
pub struct InstanceConfigurationSerializer<'a> {
    /// The row being rendered.
    pub row: &'a InstanceConfiguration,
    /// `settings.SECRET_KEY`, the input `decrypt_data` derives its key from.
    pub secret_key: &'a str,
}

impl<'a> InstanceConfigurationSerializer<'a> {
    /// `_readable_fields` order.
    pub const FIELDS: &'static [&'static str] = &[
        "id",
        "created_at",
        "updated_at",
        "deleted_at",
        "key",
        "value",
        "category",
        "is_encrypted",
        "created_by",
        "updated_by",
    ];
    /// `fields=` semantics of the base class this serializer inherits.
    pub const MODE: FilterMode = FilterMode::None;

    /// `to_representation`'s `value`: the decrypted plaintext for an encrypted
    /// non-`NULL` row, the stored text otherwise.
    #[must_use]
    pub fn value(&self) -> Option<String> {
        if self.row.is_encrypted && self.row.value.is_some() {
            Some(plane_auth::fernet::decrypt_data(
                self.secret_key,
                self.row.value.as_deref().unwrap_or_default(),
            ))
        } else {
            self.row.value.clone()
        }
    }
}

impl DrfSerialize for InstanceConfigurationSerializer<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(self.row.id))?;
        obj.field_dyn(
            "created_at",
            ctx,
            &drf_datetime(self.row.created_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "updated_at",
            ctx,
            &drf_datetime(self.row.updated_at, ctx.tz()),
        )?;
        obj.field_dyn(
            "deleted_at",
            ctx,
            &drf_datetime_opt(self.row.deleted_at, ctx.tz()),
        )?;
        obj.field_dyn("key", ctx, &self.row.key)?;
        obj.field_dyn("value", ctx, &self.value())?;
        obj.field_dyn("category", ctx, &self.row.category)?;
        obj.field_dyn("is_encrypted", ctx, &self.row.is_encrypted)?;
        obj.field_dyn("created_by", ctx, &DrfUuidOpt(self.row.created_by_id))?;
        obj.field_dyn("updated_by", ctx, &DrfUuidOpt(self.row.updated_by_id))?;
        obj.end();
        Ok(())
    }
}

/// A nullable `PrimaryKeyRelatedField`: DRF writes `null` for a `None` pk and
/// the UUID otherwise. (`Option<Uuid>`'s `Serialize` writes the same bytes; the
/// wrapper keeps the uuid formatting in one place, `DrfUuid`.)
struct DrfUuidOpt(Option<uuid::Uuid>);

impl serde::Serialize for DrfUuidOpt {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(id) => serializer.serialize_str(&id.to_string()),
            None => serializer.serialize_none(),
        }
    }
}

/// `"" if raw_value is None else str(raw_value).strip()` for the body half of
/// `InstanceConfigurationEndpoint.patch` (`configuration.py:48-49`).
///
/// A missing key is *not* this function: Python falls back to the row's own
/// value ([`stored_value`]). A field the client sends as `null` is `""`.
#[must_use]
pub fn body_value(raw: &Json) -> String {
    if raw.is_null() {
        return String::new();
    }
    python_strip(&python_str(Raw::Json(raw))).to_owned()
}

/// The same expression for the `request.data.get(key, configuration.value)`
/// fallback: a `None` column is `""`, a string is stripped.
#[must_use]
pub fn stored_value(current: Option<&str>) -> String {
    match current {
        None => String::new(),
        Some(text) => python_strip(text).to_owned(),
    }
}

/// The `value` column `PATCH` writes for one row (`configuration.py:50-54`):
///
/// ```python
/// if configuration.is_encrypted:
///     configuration.value = encrypt_data(value)
/// else:
///     configuration.value = value
/// ```
///
/// `encrypt_data("")` is `""` (`encryption.py:22-27`), so disabling a secret row
/// stores an empty **plaintext** string in the encrypted column, not a token —
/// which is also what `disable-email-feature` writes directly
/// (`configuration.py:68-79`).
#[must_use]
pub fn column_value(is_encrypted: bool, value: &str, secret_key: &str) -> String {
    if is_encrypted {
        plane_auth::fernet::encrypt_data(secret_key, value)
    } else {
        value.to_owned()
    }
}

/// CPython's `str.strip()`: `char::is_whitespace` plus U+001C..U+001F
/// (`Objects/unicodeobject.c: _PyUnicode_IsWhitespace`), the same set
/// `plane_db::hooks::user` and `users_me::write_email` use.
fn python_strip(value: &str) -> &str {
    value.trim_matches(|character: char| {
        character.is_whitespace() || matches!(character, '\u{1c}'..='\u{1f}')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(value: Option<&str>, is_encrypted: bool) -> InstanceConfiguration {
        InstanceConfiguration {
            created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("valid"),
            updated_at: chrono::DateTime::from_timestamp(1_700_000_001, 0).expect("valid"),
            created_by_id: None,
            updated_by_id: None,
            deleted_at: None,
            id: uuid::Uuid::nil(),
            key: "EMAIL_HOST_PASSWORD".to_owned(),
            value: value.map(str::to_owned),
            category: "SMTP".to_owned(),
            is_encrypted,
        }
    }

    #[test]
    fn an_encrypted_value_is_decrypted_for_the_body() {
        let secret = "plane-rs-fernet-vector-not-a-real-secret";
        let ciphertext =
            plane_auth::fernet::encrypt_data_at(secret, "hunter2", 1_700_000_000, [0u8; 16]);
        let row = row(Some(&ciphertext), true);
        let serializer = InstanceConfigurationSerializer {
            row: &row,
            secret_key: secret,
        };
        assert_eq!(serializer.value().as_deref(), Some("hunter2"));
    }

    #[test]
    fn a_plaintext_or_null_value_is_left_alone() {
        let row_plain = row(Some("smtp.example.com"), false);
        let serializer = InstanceConfigurationSerializer {
            row: &row_plain,
            secret_key: "unused",
        };
        assert_eq!(serializer.value().as_deref(), Some("smtp.example.com"));
        // An encrypted NULL stays NULL: `instance.value is not None` gates it.
        let row_null = row(None, true);
        let serializer = InstanceConfigurationSerializer {
            row: &row_null,
            secret_key: "unused",
        };
        assert_eq!(serializer.value(), None);
    }

    #[test]
    fn a_failed_decryption_is_an_empty_string() {
        let row = row(Some("not-a-token"), true);
        let serializer = InstanceConfigurationSerializer {
            row: &row,
            secret_key: "plane-rs-fernet-vector-not-a-real-secret",
        };
        assert_eq!(serializer.value().as_deref(), Some(""));
    }

    #[test]
    fn body_values_use_str_then_strip() {
        use plane_serde::pyjson;
        let cases = [
            ("\"  x  \"", "x"),
            ("\"\\u001cx\\u001f\"", "x"),
            ("\"\\u00a0x\\u3000\"", "x"),
            ("null", ""),
            ("true", "True"),
            ("false", "False"),
            ("3", "3"),
            ("1.5", "1.5"),
            ("[1, 2]", "[1, 2]"),
            ("{\"a\": \"b\"}", "{'a': 'b'}"),
        ];
        for (json, expected) in cases {
            let value = pyjson::loads(json.as_bytes(), "utf-8").expect("valid json");
            assert_eq!(body_value(&value), expected, "body {json}");
        }
    }

    #[test]
    fn stored_values_are_stripped_and_none_is_empty() {
        assert_eq!(stored_value(None), "");
        assert_eq!(stored_value(Some("  a  ")), "a");
        assert_eq!(stored_value(Some("\u{1c}a\u{1f}")), "a");
        assert_eq!(stored_value(Some("\u{a0}a\u{3000}")), "a");
        assert_eq!(stored_value(Some("\u{200b}a")), "\u{200b}a");
    }

    #[test]
    fn the_body_is_the_serializers_field_order_and_types() {
        let secret = "plane-rs-fernet-vector-not-a-real-secret";
        let ciphertext =
            plane_auth::fernet::encrypt_data_at(secret, "hunter2", 1_700_000_000, [0u8; 16]);
        let mut row = row(Some(&ciphertext), true);
        row.id = uuid::Uuid::nil();
        // Microseconds present, like the fixture rows Django serialised for the
        // comparison in the report: `isoformat()` keeps them when non-zero.
        row.created_at =
            chrono::DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("valid");
        row.created_by_id = Some(uuid::Uuid::from_u128(7));
        let serializer = InstanceConfigurationSerializer {
            row: &row,
            secret_key: secret,
        };
        let ctx = plane_serializers::RenderCtx::utc();
        let body = plane_serializers::render(&serializer, &ctx).expect("render");
        assert_eq!(
            String::from_utf8(body).expect("utf-8"),
            // DRF's field order and formats: the pk as a UUID string, a UTC
            // datetime as `…Z` (no `.000000`), a null relation as `null`, the
            // decrypted value in place of the ciphertext.
            "{\"id\":\"00000000-0000-0000-0000-000000000000\",\
             \"created_at\":\"2023-11-14T22:13:20.123456Z\",\
             \"updated_at\":\"2023-11-14T22:13:21Z\",\
             \"deleted_at\":null,\
             \"key\":\"EMAIL_HOST_PASSWORD\",\
             \"value\":\"hunter2\",\
             \"category\":\"SMTP\",\
             \"is_encrypted\":true,\
             \"created_by\":\"00000000-0000-0000-0000-000000000007\",\
             \"updated_by\":null}"
        );
    }

    #[test]
    fn strips_cpythons_whitespace_classes() {
        assert_eq!(stored_value(Some("  a@b  ")), "a@b");
        assert_eq!(stored_value(Some("\u{1c}a@b\u{1f}")), "a@b");
        assert_eq!(stored_value(Some("\u{a0}a@b\u{3000}")), "a@b");
    }

    #[test]
    fn the_value_column_is_encrypted_only_for_an_encrypted_row() {
        let secret = "plane-rs-fernet-vector-not-a-real-secret";
        assert_eq!(column_value(false, "2525", secret), "2525");
        // An empty secret is `""`, not a token (`encryption.py:22-27`).
        assert_eq!(column_value(true, "", secret), "");
        let token = column_value(true, "hunter2", secret);
        assert_eq!(plane_auth::fernet::decrypt_data(secret, &token), "hunter2");
        assert_ne!(token, "hunter2");
    }
}
