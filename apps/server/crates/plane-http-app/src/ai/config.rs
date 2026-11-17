// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `get_llm_config` (`plane/app/views/external/base.py:76-120`) — which model,
//! provider and API key the assistant uses, or "not configured".
//!
//! The three values come from `get_configuration_value` in that order —
//! `LLM_API_KEY`, `LLM_PROVIDER` (call-site default `"openai"`), `LLM_MODEL`
//! (call-site default `None`) — through **one** queryset
//! (`plane/license/utils/instance_value.py:20-23`), which is why
//! [`configuration_values`](crate::common::instance_config::configuration_values)
//! exists.
//!
//! Then, in the Python's order:
//!
//! 1. `SUPPORTED_PROVIDERS.get(provider_key.lower())` (`:98-101`). A `NULL`
//!    provider row makes `provider_key` `None` and this line an
//!    `AttributeError` (a family 500); an unknown name is the ordinary "not
//!    configured" result.
//! 2. a falsy `api_key` (`:103-105`) — empty or absent — disables the feature;
//! 3. a falsy `model` becomes the provider's `default_model` (`:107-109`);
//! 4. a model that is not in the provider's list disables it (`:111-118`).
//!
//! "Not configured" is `(None, None, None)` there and `None` here; both
//! handlers answer it with the same 400 (`:151-157`, `:187-193`).
//!
//! Two upstream details are kept as they are: the returned provider is the
//! **raw** configuration string (not the lowercased lookup key), so every error
//! message spells the operator's value, and the helper never reads
//! `OPENAI_API_KEY` — the SDK is constructed with the configured key
//! explicitly (`:131`).

use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;

use crate::common::instance_config::configuration_values;

/// One provider entry of `SUPPORTED_PROVIDERS` (`external/base.py:26-73`).
pub struct Provider {
    /// The lower-case key `SUPPORTED_PROVIDERS` is indexed by.
    pub key: &'static str,
    /// The class attribute `name`, used in error messages.
    pub name: &'static str,
    /// The models the endpoint may ask for.
    pub models: &'static [&'static str],
    /// `default_model`.
    pub default_model: &'static str,
}

pub static OPENAI: Provider = Provider {
    key: "openai",
    name: "OpenAI",
    models: &[
        "gpt-3.5-turbo",
        "gpt-4o-mini",
        "gpt-4o",
        "o1-mini",
        "o1-preview",
    ],
    default_model: "gpt-4o-mini",
};

pub static ANTHROPIC: Provider = Provider {
    key: "anthropic",
    name: "Anthropic",
    models: &[
        "claude-3-5-sonnet-20240620",
        "claude-3-haiku-20240307",
        "claude-3-opus-20240229",
        "claude-3-sonnet-20240229",
        "claude-2.1",
        "claude-2",
        "claude-instant-1.2",
        "claude-instant-1",
    ],
    default_model: "claude-3-sonnet-20240229",
};

pub static GEMINI: Provider = Provider {
    key: "gemini",
    name: "Gemini",
    models: &["gemini-pro", "gemini-1.5-pro-latest", "gemini-pro-vision"],
    default_model: "gemini-pro",
};

/// `SUPPORTED_PROVIDERS` (`external/base.py:69-73`), in declaration order.
pub static SUPPORTED_PROVIDERS: &[&Provider] = &[&OPENAI, &ANTHROPIC, &GEMINI];

impl Provider {
    /// `SUPPORTED_PROVIDERS.get(key)`.
    pub fn by_key(key: &str) -> Option<&'static Provider> {
        SUPPORTED_PROVIDERS
            .iter()
            .copied()
            .find(|provider| provider.key == key)
    }
}

/// What `get_llm_config` returns when the feature is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmConfig {
    /// `api_key` — non-empty.
    pub api_key: String,
    /// `model` — already defaulted and validated against [`Provider::models`].
    pub model: String,
    /// `provider` — the raw `LLM_PROVIDER` value, as the Python returns it.
    pub provider: String,
}

/// `get_llm_config()` (`external/base.py:76-120`).
///
/// `Ok(None)` is the Python's `(None, None, None)`: the route answers
/// `400 {"error": "LLM provider API key and model are required"}`.
///
/// # Errors
///
/// * [`ViewError::Unknown`] for the `AttributeError` a `NULL` `LLM_PROVIDER`
///   row causes at `:98` (`None.lower()`), which the family handler renders as
///   its 500 body.
/// * the database failure of the configuration read.
pub async fn llm_config(ctx: &ViewCtx<'_>) -> Result<Option<LlmConfig>, ViewError> {
    // `get_configuration_value([...])`, one queryset for all three keys.
    let values = configuration_values(
        ctx,
        &[
            ("LLM_API_KEY", None),
            ("LLM_PROVIDER", Some("openai")),
            ("LLM_MODEL", None),
        ],
    )
    .await?;
    let mut values = values.into_iter();
    let api_key = values.next().flatten();
    let provider_key = values.next().flatten();
    let model = values.next().flatten();

    // `SUPPORTED_PROVIDERS.get(provider_key.lower())` (`:98`). A `NULL` row is
    // `AttributeError: 'NoneType' object has no attribute 'lower'`, not the 400.
    let Some(provider_key) = provider_key else {
        return Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'lower'".to_string(),
        ));
    };
    let Some(provider) = Provider::by_key(&provider_key.to_lowercase()) else {
        return Ok(None);
    };

    // `if not api_key` (`:103`).
    let Some(api_key) = api_key.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    // `if not model: model = provider.default_model` (`:107-109`), then the
    // membership test at `:111-118`.
    let model = model
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| provider.default_model.to_string());
    if !provider.models.contains(&model.as_str()) {
        return Ok(None);
    }

    Ok(Some(LlmConfig {
        api_key,
        model,
        provider: provider_key,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three tables, exactly as `external/base.py:42-73` declares them.
    #[test]
    fn the_provider_tables_match_django() {
        assert_eq!(
            OPENAI.models,
            &[
                "gpt-3.5-turbo",
                "gpt-4o-mini",
                "gpt-4o",
                "o1-mini",
                "o1-preview"
            ]
        );
        assert_eq!(OPENAI.default_model, "gpt-4o-mini");
        assert_eq!(ANTHROPIC.default_model, "claude-3-sonnet-20240229");
        assert_eq!(
            ANTHROPIC.models,
            &[
                "claude-3-5-sonnet-20240620",
                "claude-3-haiku-20240307",
                "claude-3-opus-20240229",
                "claude-3-sonnet-20240229",
                "claude-2.1",
                "claude-2",
                "claude-instant-1.2",
                "claude-instant-1",
            ]
        );
        assert_eq!(GEMINI.default_model, "gemini-pro");
        assert_eq!(
            GEMINI.models,
            &["gemini-pro", "gemini-1.5-pro-latest", "gemini-pro-vision"]
        );
        assert_eq!(SUPPORTED_PROVIDERS.len(), 3);
    }

    /// `SUPPORTED_PROVIDERS.get(provider_key.lower())`: the lookup key is
    /// case-folded, the answer keeps the raw spelling.
    #[test]
    fn the_provider_lookup_is_case_insensitive() {
        assert_eq!(Provider::by_key("openai").map(|p| p.name), Some("OpenAI"));
        assert!(Provider::by_key("OpenAI").is_none());
        assert_eq!("OpenAI".to_lowercase(), "openai");
        assert_eq!(Provider::by_key("gemini").map(|p| p.name), Some("Gemini"));
        assert!(Provider::by_key("").is_none());
        assert!(Provider::by_key("azure").is_none());
    }
}
