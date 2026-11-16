// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Print a Rust presigned artefact as JSON, for
//! `apps/server/tools/storage/live_cross_check.py`.
//!
//! ```sh
//! cargo run --example presign -- post <key> <file_type|-> <file_size> <expiration|-> [now]
//! cargo run --example presign -- get <key> <disposition> <filename|-> <content_type|-> <expiration|-> [now]
//! cargo run --example presign -- export-get <key> <expiration> [now]
//! ```
//!
//! `now` is an RFC 3339 instant; it lets the Python side freeze at the same
//! instant with freezegun and compare the URL bytes without racing the clock.
//! Configuration comes from the process environment exactly as a worker would
//! read it (`AWS_*`, `USE_MINIO`, `MINIO_*`, `SIGNED_URL_EXPIRATION`, `WEB_URL`).

use chrono::{DateTime, Utc};
use plane_core::config::{ProcessEnv, Role, Settings};
use plane_storage::{PresignGet, PresignPost, Storage};

fn required(args: &[String], index: usize, name: &str) -> String {
    args.get(index)
        .cloned()
        .unwrap_or_else(|| panic!("missing argument {name}"))
}

fn optional(args: &[String], index: usize) -> Option<String> {
    args.get(index).cloned().filter(|value| value != "-")
}

fn now(args: &[String], index: usize) -> DateTime<Utc> {
    match optional(args, index) {
        Some(value) => DateTime::parse_from_rfc3339(&value)
            .expect("now is RFC 3339")
            .with_timezone(&Utc),
        None => Utc::now(),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let command = required(&args, 1, "command");
    let settings = Settings::from_source(&ProcessEnv, Role::Worker).expect("settings");
    let storage = Storage::new(&settings.storage, &ProcessEnv).expect("storage");
    let view = storage.worker();

    match command.as_str() {
        "post" => {
            let key = required(&args, 2, "key");
            let file_type = optional(&args, 3);
            let file_size: i64 = required(&args, 4, "file_size").parse().expect("file_size");
            let expiration: Option<i64> =
                optional(&args, 5).map(|value| value.parse().expect("expiration"));
            let post = view
                .presign_post_at(
                    now(&args, 6),
                    PresignPost {
                        key: &key,
                        file_type: file_type.as_deref(),
                        file_size,
                        expiration,
                    },
                )
                .await
                .expect("presign post");
            let document = serde_json::json!({
                "url": post.url,
                "fields": post.fields,
                "policy": post.policy_document(),
            });
            println!("{document}");
        }
        "get" => {
            let key = required(&args, 2, "key");
            let disposition = required(&args, 3, "disposition");
            let filename = optional(&args, 4);
            let content_type = optional(&args, 5);
            let expiration: Option<i64> =
                optional(&args, 6).map(|value| value.parse().expect("expiration"));
            let url = view
                .presign_get_at(
                    now(&args, 7),
                    PresignGet {
                        key: &key,
                        expiration,
                        disposition: &disposition,
                        filename: filename.as_deref(),
                        content_type: content_type.as_deref(),
                    },
                )
                .await
                .expect("presign get");
            println!("{}", serde_json::json!({ "url": url }));
        }
        "export-get" => {
            let key = required(&args, 2, "key");
            let expiration: i64 = required(&args, 3, "expiration")
                .parse()
                .expect("expiration");
            let url = storage
                .export_download_url_at(now(&args, 4), &key, expiration)
                .await
                .expect("export presign");
            println!("{}", serde_json::json!({ "url": url }));
        }
        other => panic!("unknown command {other}"),
    }
}
