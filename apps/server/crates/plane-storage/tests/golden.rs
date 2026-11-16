// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte comparison with boto3.
//!
//! `tests/golden/presign.json` is written by `apps/server/tools/storage/golden.py`,
//! which calls the real `S3Storage` wrapper from
//! `apps/api/plane/settings/storage.py` under a frozen clock. Every case here
//! must match the URL string, the ordered `fields` list, the base64 policy and
//! the decoded policy document exactly. Error cases must fail for the same
//! reason botocore fails.
//!
//! Regenerate with:
//!
//! ```sh
//! cd apps/server
//! ../../apps/api/.venv/bin/python tools/storage/golden.py
//! CARGO_TARGET_DIR=target/storage cargo test -p plane-storage --test golden
//! ```

use chrono::{DateTime, Utc};
use plane_core::config::{EnvMap, Role, Settings};
use plane_storage::{PresignGet, PresignPost, RequestInfo, Storage, StorageError};
use serde_json::Value;

const GOLDEN: &str = include_str!("golden/presign.json");

/// Values `plane-core`'s loader needs that have nothing to do with storage.
/// The golden file only carries the object-storage variables.
fn base_env() -> EnvMap {
    EnvMap::new()
        .set("SECRET_KEY", "test-secret-key")
        .set("REDIS_URL", "redis://127.0.0.1:56379/15")
        .set(
            "DATABASE_URL",
            "postgresql://plane:plane@127.0.0.1:55432/plane",
        )
        .set("DEBUG", "0")
        .set("FILE_SIZE_LIMIT", "5242880")
}

fn request_info(value: &Value) -> Option<RequestInfo<'static>> {
    // The golden stores `scheme:host`; the strings are owned by the parsed
    // document, which outlives every case in this test.
    let text = value.as_str()?;
    let (scheme, host) = text.split_once(':').expect("scheme:host");
    let scheme: &'static str = Box::leak(scheme.to_string().into_boxed_str());
    let host: &'static str = Box::leak(host.to_string().into_boxed_str());
    Some(RequestInfo { scheme, host })
}

fn expected_fields(value: &Value) -> Vec<(String, Option<String>)> {
    value
        .as_array()
        .expect("fields array")
        .iter()
        .map(|pair| {
            let pair = pair.as_array().expect("field pair");
            let name = pair[0].as_str().expect("field name").to_string();
            let value = pair[1].as_str().map(str::to_string);
            (name, value)
        })
        .collect()
}

#[tokio::test]
async fn presigned_artefacts_match_boto3() {
    let document: Value = serde_json::from_str(GOLDEN).expect("parse golden");
    let now: DateTime<Utc> =
        DateTime::parse_from_rfc3339(document["frozen_now"].as_str().expect("frozen_now"))
            .unwrap()
            .with_timezone(&Utc);
    let cases = document["cases"].as_array().expect("cases");
    assert!(
        cases.len() >= 60,
        "the brief requires at least 60 recorded cases, found {}",
        cases.len()
    );
    assert_eq!(
        cases.len() as u64,
        document["case_count"].as_u64().expect("case_count")
    );

    let mut outputs = 0;
    let mut error_cases = 0;

    for case in cases {
        let name = case["name"].as_str().expect("name");
        let mut env = base_env();
        for (key, value) in case["env"].as_object().expect("env") {
            if let Some(text) = value.as_str() {
                env = env.set(key, text);
            }
        }
        let settings = Settings::from_source(&env, Role::Worker)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let storage = match Storage::new(&settings.storage, &env) {
            Ok(storage) => storage,
            Err(error) => {
                // A construction-time error is only acceptable for the cases
                // boto3 also rejects when the client is built.
                assert_eq!(
                    case["expected"].get("error").and_then(Value::as_str),
                    Some(python_error_class(&error)),
                    "{name}: construction failed with {error}"
                );
                error_cases += 1;
                continue;
            }
        };
        let request = request_info(&case["request"]);
        let view = match request {
            Some(request) => storage.request(request),
            None => storage.worker(),
        };
        let input = &case["input"];
        let expected = &case["expected"];

        if expected.get("error").is_some() {
            let expected_class = expected["error"].as_str().unwrap();
            let result = if case.get("via").and_then(Value::as_str) == Some("export") {
                storage
                    .export_download_url_at(
                        now,
                        input["key"].as_str().unwrap(),
                        input["expiration"].as_i64().unwrap(),
                    )
                    .await
            } else if case["kind"] == "post" {
                view.presign_post_at(
                    now,
                    PresignPost {
                        key: input["key"].as_str().unwrap(),
                        file_type: input["file_type"].as_str(),
                        file_size: input["file_size"].as_i64().unwrap(),
                        expiration: input["expiration"].as_i64(),
                    },
                )
                .await
                .map(|post| post.url)
            } else {
                view.presign_get_at(
                    now,
                    PresignGet {
                        key: input["key"].as_str().unwrap(),
                        expiration: input["expiration"].as_i64(),
                        disposition: input["disposition"].as_str().unwrap(),
                        filename: input["filename"].as_str(),
                        content_type: input["content_type"].as_str(),
                    },
                )
                .await
            };
            match result {
                Ok(value) => panic!("{name}: expected {expected_class}, got {value:?}"),
                Err(error) => {
                    assert_error_matches(name, expected_class, &error);
                    error_cases += 1;
                    continue;
                }
            }
        }

        if case.get("via").and_then(Value::as_str) == Some("export") {
            let url = storage
                .export_download_url_at(
                    now,
                    input["key"].as_str().unwrap(),
                    input["expiration"].as_i64().unwrap(),
                )
                .await
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(url, expected["url"].as_str().unwrap(), "{name}");
            outputs += 1;
            continue;
        }

        match case["kind"].as_str().expect("kind") {
            "post" => {
                let post = view
                    .presign_post_at(
                        now,
                        PresignPost {
                            key: input["key"].as_str().unwrap(),
                            file_type: input["file_type"].as_str(),
                            file_size: input["file_size"].as_i64().unwrap(),
                            expiration: input["expiration"].as_i64(),
                        },
                    )
                    .await
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(post.url, expected["url"].as_str().unwrap(), "{name} url");
                assert_eq!(
                    post.fields,
                    expected_fields(&expected["fields"]),
                    "{name} fields"
                );
                assert_eq!(
                    post.policy_document().as_deref(),
                    expected["policy"].as_str(),
                    "{name} policy document"
                );
                assert_eq!(
                    post.policy_base64(),
                    post.fields
                        .iter()
                        .find(|(field, _)| field == "policy")
                        .and_then(|(_, value)| value.as_deref()),
                    "{name} policy field"
                );
                outputs += 1;
            }
            "get" => {
                let url = view
                    .presign_get_at(
                        now,
                        PresignGet {
                            key: input["key"].as_str().unwrap(),
                            expiration: input["expiration"].as_i64(),
                            disposition: input["disposition"].as_str().unwrap(),
                            filename: input["filename"].as_str(),
                            content_type: input["content_type"].as_str(),
                        },
                    )
                    .await
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(url, expected["url"].as_str().unwrap(), "{name}");
                outputs += 1;
            }
            other => panic!("{name}: unknown kind {other}"),
        }
    }

    assert!(outputs > 200, "only {outputs} compared outputs");
    assert_eq!(error_cases, 12, "error case count changed");
}

/// Map a [`StorageError`] to the Python exception class the golden recorded,
/// for the construction-time failures.
fn python_error_class(error: &StorageError) -> &'static str {
    match error {
        StorageError::InvalidEndpoint(_) => "ValueError",
        StorageError::InvalidRegion(_) => "InvalidRegionError",
        StorageError::PartialCredentials { .. } => "PartialCredentialsError",
        _ => "ValueError",
    }
}

fn assert_error_matches(name: &str, expected_class: &str, error: &StorageError) {
    let matches = match expected_class {
        "PartialCredentialsError" => {
            matches!(error, StorageError::PartialCredentials { .. })
        }
        "ParamValidationError" | "TypeError" => {
            matches!(error, StorageError::InvalidParameter(_))
        }
        "ValueError" => matches!(
            error,
            StorageError::InvalidEndpoint(_) | StorageError::InvalidRegion(_)
        ),
        other => panic!("{name}: unhandled Python exception class {other}"),
    };
    assert!(
        matches,
        "{name}: expected {expected_class}, got {error:?} ({error})"
    );
}

#[tokio::test]
async fn missing_filename_gets_a_random_32_hex_name() {
    // `storage.py:106-112`: a missing or non-string filename becomes
    // `uuid.uuid4().hex`. The golden file cannot record it (it changes on every
    // run), so this checks the shape and the rest of the URL.
    let mut env = base_env();
    env = env
        .set("AWS_ACCESS_KEY_ID", "AKID")
        .set("AWS_SECRET_ACCESS_KEY", "SECRET")
        .set("AWS_S3_BUCKET_NAME", "uploads")
        .set("AWS_REGION", "us-east-1")
        .set("AWS_S3_ENDPOINT_URL", "http://127.0.0.1:59000");
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    let storage = Storage::new(&settings.storage, &env).unwrap();
    for filename in [None, Some("")] {
        let url = storage
            .worker()
            .presign_get(PresignGet {
                key: "ws/a.png",
                expiration: Some(60),
                disposition: "attachment",
                filename,
                content_type: Some("image/png"),
            })
            .await
            .unwrap();
        let prefix = "http://127.0.0.1:59000/uploads/ws/a.png?response-content-disposition=attachment%3B%20filename%2A%3DUTF-8%27%27";
        let rest = url.strip_prefix(prefix).expect("URL prefix");
        let name = rest.split('&').next().unwrap();
        assert_eq!(name.len(), 32, "{url}");
        assert!(
            name.chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()),
            "{name}"
        );
    }
}
