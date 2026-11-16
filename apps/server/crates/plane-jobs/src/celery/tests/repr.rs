// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `argsrepr`/`kwargsrepr` against values produced by the real Celery 5.5.3.

use uuid::Uuid;

use crate::celery::args::TaskArg;
use crate::celery::repr::{saferepr_args, saferepr_kwargs, saferepr_value};

const UUID_TEXT: &str = "6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60";

fn uuid() -> Uuid {
    Uuid::parse_str(UUID_TEXT).expect("valid uuid")
}

#[test]
fn empty_args_and_kwargs() {
    assert_eq!(saferepr_args(&[]), "()");
    assert_eq!(saferepr_kwargs(&[]), "{}");
}

#[test]
fn one_element_tuple_has_a_trailing_comma() {
    assert_eq!(saferepr_args(&[TaskArg::str("x")]), "('x',)");
}

#[test]
fn scalars() {
    let args = vec![
        TaskArg::str("a"),
        TaskArg::Int(1),
        TaskArg::Bool(true),
        TaskArg::Bool(false),
        TaskArg::Null,
    ];
    assert_eq!(saferepr_args(&args), "('a', 1, True, False, None)");
}

#[test]
fn uuid_uses_the_python_constructor_form() {
    assert_eq!(
        saferepr_args(&[TaskArg::uuid(uuid())]),
        "(UUID('6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60'),)"
    );
    assert_eq!(
        saferepr_value(&TaskArg::list(vec![
            TaskArg::uuid(uuid()),
            TaskArg::uuid(uuid())
        ])),
        "[UUID('6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60'), \
         UUID('6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60')]"
    );
}

#[test]
fn kwargs_keep_insertion_order_and_python_separators() {
    let kwargs = vec![("a", TaskArg::Int(1)), ("b", TaskArg::str("two"))];
    assert_eq!(saferepr_kwargs(&kwargs), "{'a': 1, 'b': 'two'}");
}

#[test]
fn strings_use_celery_escaping_not_python_repr() {
    // `_format_chars` only escapes `'`; backslash, newline and tab are raw.
    let args = vec![
        TaskArg::str("it's"),
        TaskArg::str("he said \"hi\""),
        TaskArg::str("both ' and \""),
        TaskArg::str("back\\slash"),
        TaskArg::str("line\nbreak\ttab"),
    ];
    assert_eq!(
        saferepr_args(&args),
        "('it\\'s', 'he said \"hi\"', 'both \\' and \"', 'back\\slash', 'line\nbreak\ttab')"
    );
}

#[test]
fn json_text_is_a_quoted_string_of_escaped_json() {
    let text = r#"{"name": "Fix the login redirect loop", "priority": "high"}"#;
    assert_eq!(
        saferepr_value(&TaskArg::json_text(text)),
        "'{\"name\": \"Fix the login redirect loop\", \"priority\": \"high\"}'"
    );
}

#[test]
fn floats_use_python_str() {
    let args = vec![
        TaskArg::Float(1.0),
        TaskArg::Float(1e-5),
        TaskArg::Float(1e16),
        TaskArg::Float(0.1 + 0.2),
    ];
    assert_eq!(
        saferepr_args(&args),
        "(1.0, 1e-05, 1e+16, 0.30000000000000004)"
    );
}

#[test]
fn containers_are_depth_limited_at_three() {
    let deep = TaskArg::map([(
        "a",
        TaskArg::map([("b", TaskArg::map([("c", TaskArg::list([TaskArg::Int(1)]))]))]),
    )]);
    // Python: `{'a': {'b': {'c': [...]}}}` — the level-3 list collapses.
    assert_eq!(saferepr_value(&deep), "{'a': {'b': {'c': [...]}}}");
}

#[test]
fn a_budget_that_runs_out_emits_celery_s_dangling_brackets() {
    // `saferepr([1,2,3,4,5], maxlen=10)` from celery 5.5.3.
    let list = TaskArg::list((1..=5).map(TaskArg::Int));
    assert_eq!(
        crate::celery::repr::saferepr_with_maxlen(&list, 10),
        "[1, 2, 3, , ...]"
    );
    // `saferepr([[1,2,3],[4,5,6]], maxlen=8)`.
    let nested = TaskArg::list(vec![
        TaskArg::list((1..=3).map(TaskArg::Int)),
        TaskArg::list((4..=6).map(TaskArg::Int)),
    ]);
    assert_eq!(
        crate::celery::repr::saferepr_with_maxlen(&nested, 8),
        "[[1, 2, , ...]]"
    );
    // `saferepr({'aaaaaaaaaaaaaaaaaaaa': 'bbbb…', 'c': 1}, maxlen=20)`.
    let dict = TaskArg::map([
        (
            "aaaaaaaaaaaaaaaaaaaa",
            TaskArg::str("bbbbbbbbbbbbbbbbbbbbbb"),
        ),
        ("c", TaskArg::Int(1)),
    ]);
    assert_eq!(
        crate::celery::repr::saferepr_with_maxlen(&dict, 20),
        "{'aaaaaaaaaaaaaaaaaaa...', ...}"
    );
}

#[test]
fn unicode_is_not_escaped() {
    assert_eq!(
        saferepr_args(&[TaskArg::str("café"), TaskArg::str("😀")]),
        "('café', '😀')"
    );
}
