// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte comparison with the pinned `openai` SDK's HTTP request.
//!
//! `tests/golden/openai_wire.json` is written by
//! `apps/server/tools/ai/openai_wire.py`, which runs the real
//! `openai==1.63.2` client against a local capture server — the same call
//! `plane/app/views/external/base.py:131-134` makes — and records the request
//! line, the headers in arrival order, and the body bytes. Nothing here
//! contacts api.openai.com.
//!
//! What the comparison covers, and why each part matters:
//!
//! * **the body**, byte for byte: `httpx`'s `json.dumps(separators=(",", ":"),
//!   ensure_ascii=False)` over the SDK's parameter dict, whose key order is
//!   `messages` then `model` — not the order the Python view writes its
//!   keyword arguments in;
//! * **the URL join**: `OPENAI_BASE_URL` (or the default) with a trailing slash
//!   enforced and `chat/completions` appended to the path, keeping any query;
//! * **the application headers, in order**: the SDK's `_build_headers` list,
//!   without the four the HTTP transport adds (`Host`, `Accept-Encoding`,
//!   `Connection`, `Content-Length`). An extra, missing or reordered SDK header
//!   fails the test.
//!
//! Three headers describe the *machine* the SDK ran on, not the protocol:
//! `X-Stainless-OS`, `-Arch` and `-Runtime-Version`. The Rust binary computes
//! the first two for its own platform (the same machine in a deployment) and
//! pins the third to the interpreter the golden recorded; those three are
//! checked against the recorded platform instead of against a literal.
//!
//! Regenerate the golden when the `openai` pin in `apps/api/pyproject.toml`
//! moves:
//!
//! ```sh
//! cd apps/server
//! ../api/.venv/bin/python tools/ai/openai_wire.py
//! ```

use plane_http_app::ai::http::{PYTHON_RUNTIME_VERSION, SDK_VERSION, prepare};
use serde_json::Value;

const GOLDEN: &str = include_str!("golden/openai_wire.json");

/// The headers httpx's transport adds; the SDK's own list is what this crate
/// builds.
const TRANSPORT_HEADERS: &[&str] = &["host", "accept-encoding", "connection", "content-length"];

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden/openai_wire.json must be valid JSON")
}

/// `platform.system()` → the Rust `target_os` value, so the comparison can tell
/// whether the golden was recorded on the platform this test runs on.
fn rust_os(system: &str) -> Option<&'static str> {
    match system {
        "Darwin" => Some("macos"),
        "Linux" => Some("linux"),
        "Windows" => Some("windows"),
        _ => None,
    }
}

/// `get_architecture()`'s value → the Rust `target_arch` value.
fn rust_arch(arch: &str) -> Option<&'static str> {
    match arch {
        "arm64" => Some("aarch64"),
        "x64" => Some("x86_64"),
        "x32" => Some("x86"),
        "arm" => Some("arm"),
        _ => None,
    }
}

#[test]
fn the_pins_match_the_golden() {
    let golden = golden();
    assert_eq!(
        SDK_VERSION,
        golden["sdk"]["openai"].as_str().expect("sdk.openai"),
        "the SDK version changed: regenerate the golden and bump SDK_VERSION together"
    );
    assert_eq!(
        PYTHON_RUNTIME_VERSION,
        golden["sdk"]["python"].as_str().expect("sdk.python"),
        "the interpreter that recorded the golden changed: bump PYTHON_RUNTIME_VERSION"
    );
}

#[test]
fn every_recorded_request_is_reproduced_byte_for_byte() {
    let golden = golden();
    let api_key = golden["api_key"].as_str().expect("api_key");
    let platform_system = golden["sdk"]["platform_system"]
        .as_str()
        .expect("platform_system");
    let platform_machine = golden["sdk"]["platform_machine"]
        .as_str()
        .expect("platform_machine");
    let same_platform = rust_os(platform_system) == Some(std::env::consts::OS);
    let same_arch = rust_arch(platform_machine) == Some(std::env::consts::ARCH);

    let cases = golden["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "the golden has no cases");
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let env = &case["env"];
        let base_url = case["base_url"]
            .as_str()
            .expect("base_url")
            .replace("{port}", "65535");
        let model = case["model"].as_str().expect("model");
        let task = case["task"].as_str().expect("task");
        let prompt = case["prompt"].as_str().expect("prompt");
        // `final_text = task + "\n" + prompt` (`external/base.py:125`).
        let content = format!("{task}\n{prompt}");

        let organization = env.get("OPENAI_ORG_ID").and_then(Value::as_str);
        let project = env.get("OPENAI_PROJECT_ID").and_then(Value::as_str);
        let wire = prepare(
            &base_url,
            api_key,
            model,
            &content,
            organization,
            project,
            0,
        );

        let request = &case["request"];
        assert_eq!(request["method"].as_str(), Some("POST"), "{name}");

        // The request target, i.e. what httpx's base-URL join produces.
        let target = plane_net::split_target(&wire.url).unwrap_or_else(|error| {
            panic!(
                "{name}: {wire_url} is not a URL: {error}",
                wire_url = wire.url
            )
        });
        assert_eq!(
            target.path,
            request["target"].as_str().expect("target"),
            "{name}: the request path"
        );

        // The SDK's headers, minus the transport's, in httpx's order.
        let recorded: Vec<(String, String)> = request["headers"]
            .as_array()
            .expect("headers")
            .iter()
            .map(|pair| {
                (
                    pair[0].as_str().expect("header name").to_string(),
                    pair[1].as_str().expect("header value").to_string(),
                )
            })
            .filter(|(header, _)| {
                !TRANSPORT_HEADERS
                    .iter()
                    .any(|transport| header.eq_ignore_ascii_case(transport))
            })
            .collect();
        assert_eq!(
            wire.headers.len(),
            recorded.len(),
            "{name}: header count. ours: {:#?}\nthe sdk's: {recorded:#?}",
            wire.headers
        );
        for (index, (ours, theirs)) in wire.headers.iter().zip(&recorded).enumerate() {
            assert_eq!(
                ours.0.to_lowercase(),
                theirs.0.to_lowercase(),
                "{name}: header #{index} name (order is part of the contract)"
            );
            let platform_header = ours.0.eq_ignore_ascii_case("X-Stainless-OS")
                || ours.0.eq_ignore_ascii_case("X-Stainless-Arch");
            if platform_header && !(same_platform && same_arch) {
                // The header describes the recording machine; on another
                // platform the literal is expected to differ.
                continue;
            }
            assert_eq!(ours.1, theirs.1, "{name}: header {} value", ours.0);
        }

        // The body, byte for byte.
        assert_eq!(
            String::from_utf8(wire.body.clone()).expect("the body is utf-8"),
            request["body"].as_str().expect("body"),
            "{name}: the JSON body"
        );
        assert_eq!(
            wire.body.len(),
            usize::try_from(request["body_bytes"].as_u64().expect("body_bytes"))
                .expect("body_bytes fits"),
            "{name}: body length"
        );
    }
}

/// `OPENAI_ORG_ID` / `OPENAI_PROJECT_ID` are read per request, and each is
/// omitted (not empty) when unset.
#[test]
fn the_organization_headers_appear_only_when_configured() {
    let wire = prepare(
        "https://api.openai.com/v1",
        "k",
        "gpt-4o-mini",
        "c",
        None,
        Some("proj"),
        0,
    );
    let names: Vec<&str> = wire.headers.iter().map(|(name, _)| name.as_str()).collect();
    assert!(!names.contains(&"OpenAI-Organization"));
    assert!(names.contains(&"OpenAI-Project"));
}
