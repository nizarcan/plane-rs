// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared test helpers: the corpus's annotated value format.
//!
//! `tools/cache/pickle_corpus.py` records every value twice: `tree` keeps the
//! object identity the pickler's memo needs (`{"r": n}` for a repeat
//! reference) and the `ReturnList`/`ReturnDict` wrappers, while `value` is
//! what `pickle.loads` gives back. [`build`] turns either into a [`PyValue`].

#![allow(dead_code)]

use std::sync::Arc;

use chrono::{NaiveDate, NaiveTime, TimeDelta};
use plane_cache::value::{PyDateTime, PyMapping, PyReduce, PyTz};
use plane_cache::{PyInt, PyValue, ReduceFunc};
use plane_serde::PyDecimal;
use plane_serde::pyjson::PyStr;
use serde_json::Value as Json;

/// `PyValue: PartialEq` compares `f64`s, and `NaN != NaN`; the corpus has a
/// NaN case.
pub fn same(left: &PyValue, right: &PyValue) -> bool {
    match (left, right) {
        (PyValue::Float(left), PyValue::Float(right)) => {
            (left.is_nan() && right.is_nan()) || left == right
        }
        (PyValue::List(left), PyValue::List(right))
        | (PyValue::Tuple(left), PyValue::Tuple(right))
        | (PyValue::Set(left), PyValue::Set(right))
        | (PyValue::FrozenSet(left), PyValue::FrozenSet(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| same(left, right))
        }
        (PyValue::Dict(left), PyValue::Dict(right)) => {
            left.entries.len() == right.entries.len()
                && left
                    .entries
                    .iter()
                    .zip(right.entries.iter())
                    .all(|((lk, lv), (rk, rv))| same(lk, rk) && same(lv, rv))
        }
        (PyValue::DateTime(left), PyValue::DateTime(right)) => {
            left.naive == right.naive && left.fold == right.fold && left.tz == right.tz
        }
        _ => left == right,
    }
}

/// Build a [`PyValue`] from the corpus's annotated tree, resolving `{"r": n}`
/// references through `table`.
pub fn build(node: Json, table: &mut Vec<Option<PyValue>>) -> PyValue {
    match &node {
        Json::Null => return PyValue::None,
        Json::Bool(value) => return PyValue::Bool(*value),
        _ => {}
    }
    let object = node.as_object().expect("a node is an object");
    if let Some(index) = object.get("r") {
        let index = index.as_u64().expect("ref index") as usize;
        return table[index]
            .clone()
            .unwrap_or_else(|| panic!("reference {index} before its definition"));
    }
    let index = if memoised(object) {
        let index = table.len();
        table.push(None);
        Some(index)
    } else {
        None
    };
    let value = build_inner(object, table);
    if let Some(index) = index {
        table[index] = Some(value.clone());
    }
    value
}

/// The tags CPython's pickler memoises: `str`, `bytes`, every container, and
/// the `__reduce__`-based scalars. An empty tuple is not memoised.
fn memoised(object: &serde_json::Map<String, Json>) -> bool {
    match object.keys().next().map(String::as_str) {
        Some(
            "s" | "s8" | "b" | "d" | "e" | "z" | "u" | "D" | "T" | "H" | "X" | "Z" | "M" | "L",
        ) => true,
        Some("l") => true,
        // `{"t": []}` is the empty tuple, which `save_tuple` does not memoise.
        Some("t") => object["t"]
            .as_array()
            .is_some_and(|items| !items.is_empty()),
        _ => false,
    }
}

fn build_inner(
    object: &serde_json::Map<String, Json>,
    table: &mut Vec<Option<PyValue>>,
) -> PyValue {
    let (tag, payload) = object.iter().next().expect("one tag");
    match tag.as_str() {
        "i" => PyValue::Int(PyInt::parse(payload.as_str().expect("int text")).expect("int")),
        "f" => PyValue::Float(parse_float(payload.as_str().expect("float text"))),
        "s" => PyValue::string(payload.as_str().expect("string").to_string()),
        "s8" => {
            let bytes = unhex(payload.as_str().expect("wtf8 hex"));
            PyValue::Str(Arc::new(PyStr::from_wtf8(bytes).expect("valid WTF-8")))
        }
        "b" => PyValue::bytes(unhex(payload.as_str().expect("bytes hex"))),
        "l" => PyValue::List(Arc::new(build_list(payload, table))),
        "t" => PyValue::Tuple(Arc::new(build_list(payload, table))),
        "e" => PyValue::Set(Arc::new(build_list(payload, table))),
        "z" => PyValue::FrozenSet(Arc::new(build_list(payload, table))),
        "d" => PyValue::Dict(Arc::new(PyMapping {
            entries: payload
                .as_array()
                .expect("dict entries")
                .iter()
                .map(|pair| (build(pair[0].clone(), table), build(pair[1].clone(), table)))
                .collect(),
        })),
        "u" => PyValue::uuid(payload.as_str().expect("uuid").parse().expect("valid uuid")),
        "D" => PyValue::date(
            NaiveDate::parse_from_str(payload.as_str().expect("date"), "%Y-%m-%d")
                .expect("valid date"),
        ),
        "T" => {
            let naive = payload["n"].as_str().expect("naive datetime");
            let fold = payload["fold"].as_bool().expect("fold");
            let naive = chrono::NaiveDateTime::parse_from_str(naive, "%Y-%m-%dT%H:%M:%S%.f")
                .expect("valid datetime");
            let tz = match &payload["tz"] {
                Json::Null => None,
                // The datetime's tzinfo is a timezone node: `{"Z": …}`.
                other => Some(Arc::new(parse_tz(&other["Z"]))),
            };
            PyValue::DateTime(Arc::new(PyDateTime { naive, tz, fold }))
        }
        "H" => PyValue::Time(Arc::new(
            NaiveTime::parse_from_str(payload.as_str().expect("time"), "%H:%M:%S%.f")
                .expect("valid time"),
        )),
        "X" => {
            let days = payload["days"].as_i64().expect("days");
            let seconds = payload["seconds"].as_i64().expect("seconds");
            let micros = payload["microseconds"].as_i64().expect("microseconds");
            PyValue::TimeDelta(Arc::new(
                TimeDelta::days(days)
                    + TimeDelta::seconds(seconds)
                    + TimeDelta::microseconds(micros),
            ))
        }
        "Z" => PyValue::TimeZone(Arc::new(parse_tz(payload))),
        "M" => PyValue::Decimal(Arc::new(
            PyDecimal::parse(payload.as_str().expect("decimal")).expect("valid decimal"),
        )),
        "L" => {
            let func = match payload["func"].as_str().expect("func") {
                "list" => ReduceFunc::List,
                "dict" => ReduceFunc::Dict,
                other => panic!("unknown reduce func {other}"),
            };
            PyValue::Reduce(Arc::new(PyReduce {
                func,
                value: build(payload["v"].clone(), table),
            }))
        }
        other => panic!("unknown tag {other}"),
    }
}

fn parse_tz(node: &Json) -> PyTz {
    match node {
        Json::String(text) => {
            assert_eq!(text, "utc");
            PyTz::Utc
        }
        Json::Number(number) => PyTz::Offset(number.as_i64().expect("offset") as i32),
        other => panic!("unknown timezone node {other:?}"),
    }
}

fn build_list(payload: &Json, table: &mut Vec<Option<PyValue>>) -> Vec<PyValue> {
    payload
        .as_array()
        .expect("array")
        .iter()
        .map(|item| build(item.clone(), table))
        .collect()
}

/// Python's `repr(float)` is what the corpus records; Rust's parser accepts
/// the same spellings including `inf`, `-inf` and `nan`.
fn parse_float(text: &str) -> f64 {
    match text {
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        "nan" => f64::NAN,
        other => other.parse().expect("float"),
    }
}

pub fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digits: String = pair.iter().map(|byte| *byte as char).collect();
            u8::from_str_radix(&digits, 16).expect("hex")
        })
        .collect()
}
