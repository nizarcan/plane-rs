// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `?lang=` for the OpenAPI document.
//!
//! `SpectacularAPIView.get` wraps generation in `translation.override(lang)`
//! when `settings.USE_I18N` and the parameter is non-empty
//! (`drf_spectacular/views.py:82-88`). The v1 document has exactly one
//! translatable message: DRF's UUID parameter description, rendered three times
//! (`rest_framework/schemas/utils.py:31-40`). `translations.json` is recorded
//! from Django by `tools/openapi/capture.py`; it holds the source string, the
//! translation for every locale directory that defines one, and a probe table
//! that pins the resolution below.
//!
//! ## Resolution
//!
//! Django turns the parameter into a gettext locale with `to_locale()` and then
//! lets CPython's `gettext._expand_lang()` choose the catalog
//! (`django/utils/translation/trans_real.py:191-204`). For the shapes a client
//! sends, that expansion tries the full code and then the bare language — it
//! never tries an intermediate component: `de-DE-x` reaches `de`, while
//! `pt-br-x` reaches `pt` (which has no translation), not `pt-br`. The
//! resolved catalog string is replaced in the committed document.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

/// `deploy/openapi/translations.json`, generated from Django.
const TABLE: &str = include_str!("../../../../deploy/openapi/translations.json");

#[derive(Deserialize)]
struct Translations {
    source: String,
    languages: BTreeMap<String, String>,
    /// Read by `probes_match_django`; kept in the artifact so the oracle and
    /// the table are regenerated together.
    #[allow(dead_code)]
    probes: BTreeMap<String, Option<String>>,
}

static TABLE_PARSED: LazyLock<Translations> = LazyLock::new(|| {
    serde_json::from_str(TABLE)
        .unwrap_or_else(|error| panic!("deploy/openapi/translations.json is not valid: {error}"))
});

/// The translated UUID help text for `lang`, or the source string.
#[must_use]
pub fn help_text(lang: &str) -> &str {
    resolve(lang).unwrap_or(&TABLE_PARSED.source)
}

fn resolve(lang: &str) -> Option<&str> {
    // `to_language()`/`to_locale()` lower-case and use `_`; gettext's
    // `locale.normalize()` drops a `.charset`/`@modifier` suffix.
    let lower = lang.to_lowercase().replace('_', "-");
    let stem = lower.split(['.', '@']).next().unwrap_or(&lower);
    let parts: Vec<&str> = stem.split('-').filter(|part| !part.is_empty()).collect();
    let candidates: Vec<String> = match parts.as_slice() {
        [] => return None,
        [one] => vec![(*one).to_owned()],
        [one, two] => vec![format!("{one}-{two}"), (*one).to_owned()],
        // A tail (`de-DE-x`) makes `_expand_lang` yield the whole code and the
        // bare language only.
        _ => vec![stem.to_owned(), parts[0].to_owned()],
    };
    candidates
        .iter()
        .find_map(|candidate| TABLE_PARSED.languages.get(candidate).map(String::as_str))
}

/// Replace the translatable message in a committed document.
#[must_use]
pub fn translate(body: &[u8], lang: &str) -> Vec<u8> {
    let source = TABLE_PARSED.source.as_bytes();
    let translated = help_text(lang);
    if translated.as_bytes() == source {
        return body.to_vec();
    }
    replace_all(body, source, translated.as_bytes())
}

fn replace_all(haystack: &[u8], needle: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(position) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        out.extend_from_slice(&rest[..position]);
        out.extend_from_slice(replacement);
        rest = &rest[position + needle.len()..];
    }
    out.extend_from_slice(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_match_django() {
        for (probe, expected) in &TABLE_PARSED.probes {
            let expected = expected.as_deref().unwrap_or(&TABLE_PARSED.source);
            assert_eq!(help_text(probe), expected, "lang={probe:?}");
        }
    }

    #[test]
    fn replacing_the_message_keeps_everything_else() {
        let body =
            b"a A UUID string identifying this Sticky. b A UUID string identifying this Sticky.";
        let translated = translate(body, "de");
        let text = String::from_utf8(translated).expect("utf8");
        assert_eq!(
            text,
            "a Ein UUID-String, der Sticky identifiziert. b Ein UUID-String, der Sticky identifiziert."
        );
        assert_eq!(translate(body, "en"), body);
    }
}
