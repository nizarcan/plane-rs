// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Django-template-compatible rendering of the e-mail bodies.
//!
//! `render_to_string("emails/…", context)` with Django's template engine is
//! replaced by minijinja (DECISIONS.md D12) over checked-in `.j2` copies. The
//! templates live in `plane-mail/templates/`; the crate embeds them with
//! `include_str!` so a distroless image needs no files (CUTOVER.md §3).
//!
//! Two behaviours are Django's, not Jinja's:
//!
//! * **Autoescaping.** Every template is HTML. Django's `escape`
//!   (`django/utils/html.py`) and minijinja's HTML escaper produce the same five
//!   replacements (`&`, `<`, `>`, `"`, `'` → `&amp;`, `&lt;`, `&gt;`,
//!   `&quot;`, `&#x27;`), proven by the golden corpus.
//! * **Value formatting.** Django renders `{{ value }}` as Python `str(value)`
//!   after autoescaping: `None` → `"None"`, `True` → `"True"`, `1` → `"1"`.
//!   minijinja's default formatter prints `none`/`true`, so
//!   [`python_formatter`] rewrites the value first and then defers to
//!   minijinja's escaper.
//!
//! Only the templates a task renders are registered; [`render`] returns
//! [`TemplateError::UnknownTemplate`] for the rest rather than guessing, which
//! keeps a half-ported set from emitting a blank e-mail.

use std::sync::LazyLock;

use minijinja::value::{Serde, Value, ValueKind};
use minijinja::{AutoEscape, Environment};
use serde_json::Value as Json;

/// The Django template names this crate can render, with their checked-in
/// `.j2` copies. A later task set appends its template here and adds the file.
const TEMPLATES: &[(&str, &str)] = &[
    (
        "emails/auth/forgot_password.html",
        include_str!("../templates/forgot_password.html.j2"),
    ),
    (
        "emails/auth/magic_signin.html",
        include_str!("../templates/magic_signin.html.j2"),
    ),
    (
        "emails/exports/analytics.html",
        include_str!("../templates/analytics.html.j2"),
    ),
    (
        "emails/notifications/webhook-deactivate.html",
        include_str!("../templates/webhook_deactivate.html.j2"),
    ),
    (
        "emails/notifications/issue-updates.html",
        include_str!("../templates/issue_updates.html.j2"),
    ),
    (
        "emails/notifications/project_addition.html",
        include_str!("../templates/project_addition.html.j2"),
    ),
    (
        "emails/invitations/workspace_invitation.html",
        include_str!("../templates/workspace_invitation.html.j2"),
    ),
    (
        "emails/user/user_activation.html",
        include_str!("../templates/user_activation.html.j2"),
    ),
    (
        "emails/user/user_deactivation.html",
        include_str!("../templates/user_deactivation.html.j2"),
    ),
    (
        "emails/user/email_updated.html",
        include_str!("../templates/email_updated.html.j2"),
    ),
];

/// The rendering error a task reports as a handler failure.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// The Django template name has not been ported yet.
    #[error("e-mail template {0} has not been ported to minijinja")]
    UnknownTemplate(String),
    /// A syntax, lookup or formatting error inside minijinja.
    #[error("{0}")]
    Render(#[from] minijinja::Error),
}

/// `render_to_string(name, context)`.
///
/// `context` is a JSON object, the same shape a Celery payload carries.
///
/// # Errors
///
/// [`TemplateError::UnknownTemplate`] when `name` is not in [`TEMPLATES`], or
/// any minijinja error (undefined is lenient, as Django's default missing
/// variable renders the empty string).
pub fn render(name: &str, context: &Json) -> Result<String, TemplateError> {
    let environment = environment();
    let template = environment
        .get_template(name)
        .map_err(|_| TemplateError::UnknownTemplate(name.to_string()))?;
    Ok(template.render(Value::from(Serde(context)))?)
}

/// [`render`] against a caller-supplied environment, for golden tests that
/// need to override a filter.
///
/// # Errors
///
/// See [`render`].
pub fn render_with_environment(
    environment: &Environment,
    name: &str,
    context: &Json,
) -> Result<String, TemplateError> {
    let template = environment
        .get_template(name)
        .map_err(|_| TemplateError::UnknownTemplate(name.to_string()))?;
    Ok(template.render(Value::from(Serde(context)))?)
}

/// The process-wide environment. Built once; templates never change at runtime.
#[must_use]
pub fn environment() -> &'static Environment<'static> {
    static ENVIRONMENT: LazyLock<Environment<'static>> = LazyLock::new(|| {
        let mut environment = Environment::new();
        environment.set_syntax(
            minijinja::syntax::SyntaxConfig::builder()
                .trim_blocks(false)
                .lstrip_blocks(false)
                .keep_trailing_newline(true)
                .build()
                .expect("static syntax configuration"),
        );
        // Django templates default to HTML autoescaping for `render_to_string`
        // (`django/template/backends/django.py:47-53` sets autoescape=True).
        environment.set_auto_escape_callback(|_| AutoEscape::Html);
        // Django's variable resolution never fails: `a.b.c` on a missing `b`
        // is `string_if_invalid` (""), and a deep template such as
        // `issue-updates.html` relies on that (`update.changes.blocking…`
        // when the update has no `blocking` key). `minijinja`'s default
        // lenient behaviour still rejects an attribute access *on* undefined,
        // so the chainable behaviour is the Django-compatible one.
        environment.set_undefined_behavior(minijinja::UndefinedBehavior::Chainable);
        environment.set_formatter(python_formatter);
        // `issue-updates.html` uses three Django filters that Jinja has no
        // equivalent for: `slice` (Django's is a Python slice spec, Jinja's
        // partitions a list), `add` (int addition with a string argument) and
        // `striptags` (`django.utils.html.strip_tags`). They are registered
        // for the whole environment because the templates are Django's.
        environment.add_filter("slice", django_slice);
        environment.add_filter("add", django_add);
        environment.add_filter("striptags", django_striptags);
        for (name, source) in TEMPLATES {
            environment
                .add_template_owned((*name).to_string(), (*source).to_string())
                .expect("checked-in e-mail templates parse");
        }
        environment
    });
    &ENVIRONMENT
}

/// `django.template.defaultfilters.slice` (`defaultfilters.py:1068-1082`): a
/// Python slice spec (`":2"`, `"1:"`, `"-2:"`, `"::2"`), not Jinja's list
/// partitioner. A spec that does not parse, or a value Python cannot slice,
/// returns the value unchanged.
fn django_slice(value: Value, spec: Value) -> Value {
    let spec = python_str(&spec);
    let mut parts = spec.split(':');
    let parse = |part: Option<&str>| -> Option<Option<i64>> {
        match part {
            // A missing component is Python's `None` bound, not a failure.
            None | Some("") => Some(None),
            Some(text) => text.trim().parse::<i64>().ok().map(Some),
        }
    };
    let Some(start) = parse(parts.next()) else {
        return value;
    };
    let Some(stop) = parse(parts.next()) else {
        return value;
    };
    let step = match parse(parts.next()) {
        Some(step) => step,
        None => return value,
    };
    if parts.next().is_some() {
        return value;
    }
    if let Some(text) = value.as_str() {
        let chars: Vec<char> = text.chars().collect();
        let Some((begin, end, stride)) = python_slice_indices(chars.len(), start, stop, step)
        else {
            return value;
        };
        let mut out = String::new();
        let mut index = begin;
        while (stride > 0 && index < end) || (stride < 0 && index > end) {
            out.push(chars[index as usize]);
            index += stride;
        }
        return Value::from(out);
    }
    let Ok(items) = value.try_iter() else {
        return value;
    };
    let items: Vec<Value> = items.collect();
    let Some((begin, end, stride)) = python_slice_indices(items.len(), start, stop, step) else {
        return value;
    };
    let mut out = Vec::new();
    let mut index = begin;
    while (stride > 0 && index < end) || (stride < 0 && index > end) {
        out.push(items[index as usize].clone());
        index += stride;
    }
    Value::from(out)
}

/// `slice.indices(len)`: the concrete `(start, stop, step)` a Python slice
/// selects, or `None` for a zero step (a `ValueError` Django catches).
fn python_slice_indices(
    length: usize,
    start: Option<i64>,
    stop: Option<i64>,
    step: Option<i64>,
) -> Option<(i64, i64, i64)> {
    let step = step.unwrap_or(1);
    if step == 0 {
        return None;
    }
    let length = length as i64;
    let adjust = |value: i64| -> i64 {
        if value < 0 {
            (value + length).max(if step > 0 { 0 } else { -1 })
        } else {
            value.min(if step > 0 { length } else { length - 1 })
        }
    };
    let start = start.map_or_else(|| if step > 0 { 0 } else { length - 1 }, adjust);
    let stop = stop.map_or_else(|| if step > 0 { length } else { -1 }, adjust);
    Some((start, stop, step))
}

/// `django.template.defaultfilters.add` (`defaultfilters.py:1471-1479`):
/// `int(value) + int(arg)`, falling back to `value + arg` (strings, lists)
/// and then to the empty string.
fn django_add(value: Value, arg: Value) -> Value {
    let as_int = |value: &Value| -> Option<i64> { python_str(value).trim().parse::<i64>().ok() };
    if let (Some(left), Some(right)) = (as_int(&value), as_int(&arg)) {
        return Value::from(left + right);
    }
    if let (Some(left), Some(right)) = (value.as_str(), arg.as_str()) {
        return Value::from(format!("{left}{right}"));
    }
    if let (Ok(left), Ok(right)) = (value.try_iter(), arg.try_iter()) {
        let mut items: Vec<Value> = left.collect();
        items.extend(right);
        return Value::from(items);
    }
    Value::from("")
}

/// `django.template.defaultfilters.striptags` (`defaultfilters.py:1817-1820`)
/// = `django.utils.html.strip_tags`, already ported for the plain-text body
/// (`plane_mail::plain_text`).
fn django_striptags(value: Value) -> Value {
    Value::from(crate::plain_text::strip_tags(&python_str(&value)))
}

/// Django's `render_value_in_context` (`django/template/base.py:1015-1028`):
/// `str(value)` under autoescape, including Python's spellings for `None`,
/// booleans, sequences and mappings.
fn python_formatter(
    out: &mut minijinja::Output,
    state: &mut minijinja::State,
    value: &Value,
) -> Result<(), minijinja::Error> {
    let text = python_str(value);
    // A value marked safe (`|safe`) is written raw, as Django's
    // `conditional_escape` does; `SafeString` is Django's analogue.
    if matches!(state.auto_escape(), AutoEscape::Html) && !value.is_safe() {
        write_django_escaped(out, &text)?;
    } else {
        out.write_str(&text)?;
    }
    Ok(())
}

/// `django.utils.html.escape` (`django/utils/html.py:50-59`):
/// `html.escape(str(value))` with `quote=True`.
///
/// `html.escape` replaces exactly `&`, `<`, `>`, `"` and `'`; it does **not**
/// escape `/`, which minijinja's own escaper does, so this is written out.
fn write_django_escaped(out: &mut minijinja::Output, text: &str) -> Result<(), minijinja::Error> {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#x27;"),
            other => escaped.push(other),
        }
    }
    out.write_str(&escaped)?;
    Ok(())
}

/// Python `str()` for the JSON values a template context carries.
fn python_str(value: &Value) -> String {
    match value.kind() {
        // Django's `string_if_invalid` is the empty string: a name the context
        // does not define renders nothing, while a defined `None` renders
        // "None" (`render_value_in_context`).
        ValueKind::Undefined => String::new(),
        ValueKind::None => "None".to_string(),
        ValueKind::Bool => {
            if value == &Value::from(true) {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        ValueKind::String => value.as_str().unwrap_or_default().to_string(),
        ValueKind::Seq => {
            let mut parts = Vec::new();
            if let Ok(iter) = value.try_iter() {
                for item in iter {
                    parts.push(python_repr(&item));
                }
            }
            format!("[{}]", parts.join(", "))
        }
        ValueKind::Map => {
            let mut parts = Vec::new();
            if let Ok(iter) = value.try_iter() {
                for key in iter {
                    let Some(key_text) = key.as_str().map(str::to_string) else {
                        continue;
                    };
                    let item = value.get_item(&key).unwrap_or(Value::UNDEFINED);
                    parts.push(format!("'{key_text}': {}", python_repr(&item)));
                }
            }
            format!("{{{}}}", parts.join(", "))
        }
        // Numbers, bytes and plain objects: minijinja's Display is Python's
        // spelling for every integer the templates use. A float with an
        // exponent would need Python's `repr`; no current template has one.
        _ => value.to_string(),
    }
}

/// Python `repr()` for a list element (`str(['a'])` -> `['a']`).
fn python_repr(value: &Value) -> String {
    if matches!(value.kind(), ValueKind::String) {
        let text = value.as_str().unwrap_or_default();
        let escaped = text.replace('\\', "\\\\").replace('\'', "\\'");
        format!("'{escaped}'")
    } else {
        python_str(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_templates_are_an_explicit_error() {
        assert!(matches!(
            render("emails/not/ported.html", &Json::Null),
            Err(TemplateError::UnknownTemplate(_))
        ));
    }

    #[test]
    fn escaping_matches_django() {
        let html = render(
            "emails/auth/magic_signin.html",
            &serde_json::json!({"code": "<&\"'>", "email": "a@b.c"}),
        )
        .expect("render");
        assert!(html.contains("&lt;&amp;&quot;&#x27;&gt;"));
        assert!(!html.contains("<&\"'>"));
    }

    #[test]
    fn values_use_python_spellings() {
        assert_eq!(python_str(&Value::from(1)), "1");
        assert_eq!(python_str(&Value::from(true)), "True");
        assert_eq!(python_str(&Value::from(())), "None");
    }
}
