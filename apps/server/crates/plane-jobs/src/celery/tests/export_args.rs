// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `argsrepr`/`kwargsrepr` for the two export constructors' raw body values.
//!
//! `issue_export_task` takes `project_ids`/`multiple` and `analytic_export_task`
//! takes `data` from `request.data` (`plane/app/views/exporter/base.py:29-30`,
//! `:50`; `plane/app/views/analytic/base.py:256`). A JSON body can carry a
//! string, a list, an int or a nested value where the nominal Python type says
//! list/bool/dict, and the golden corpus in `tools/celery/golden/messages.json`
//! records what Celery's `saferepr` produces for each. This test isolates the
//! two headers from the rest of the message so a divergence names the value.
//!
//! The expected strings are read from the recorder's output, not retyped here:
//! `tools/celery/cases.py` publishes the same values through the real Celery
//! task wrapper (`record.py`).

use serde_json::Value;

use crate::celery::args::{argsrepr, kwargsrepr};
use crate::celery::test_cases;

const GOLDEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/celery/golden/messages.json"
));

/// The five raw-body cases appended to `cases.py` for this constructor pair.
const RAW_CASES: &[&str] = &[
    "issue_export_task_raw_string",
    "issue_export_task_raw_ints",
    "issue_export_task_raw_nested",
    "analytic_export_task_raw_list",
    "analytic_export_task_raw_int",
];

#[test]
fn raw_body_values_render_the_saferepr_celery_recorded() {
    let document: Value = serde_json::from_str(GOLDEN).expect("golden/messages.json parses");
    let messages = document["messages"]
        .as_array()
        .expect("messages is an array");

    for case in RAW_CASES {
        let recorded = messages
            .iter()
            .find(|message| message["case"].as_str() == Some(*case))
            .unwrap_or_else(|| panic!("golden has no case {case}"));
        let message =
            test_cases::build(case).unwrap_or_else(|| panic!("no Rust builder for {case}"));
        let headers = &recorded["headers"];
        assert_eq!(
            argsrepr(message.args()),
            headers["argsrepr"].as_str().expect("argsrepr"),
            "argsrepr for {case}"
        );
        assert_eq!(
            kwargsrepr(message.kwargs()),
            headers["kwargsrepr"].as_str().expect("kwargsrepr"),
            "kwargsrepr for {case}"
        );
    }
}
