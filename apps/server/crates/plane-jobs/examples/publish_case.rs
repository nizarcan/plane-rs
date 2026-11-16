// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Publish one recorded case to a real broker, with the identity pinned the
//! way `tools/celery/record.py` pins it.
//!
//! ```sh
//! cargo run -p plane-jobs --features test-cases --example publish_case -- \
//!     --url amqp://plane:plane@127.0.0.1:55672/t_celery_live --case process_logs
//! ```
//!
//! The task id is derived from the case's position in
//! `celery::test_cases::case_ids`, exactly as `cases.numbered()` does, so a
//! Python publish of the same case produces the same bytes.

use std::error::Error;

use chrono::{DateTime, Utc};
use plane_jobs::celery::message::EncodeContext;
use plane_jobs::celery::publisher::CeleryPublisher;
use plane_jobs::celery::test_cases;
use uuid::Uuid;

const PINNED_NOW: &str = "2026-11-15T17:24:04.144120+00:00";
const PINNED_ORIGIN: &str = "gen4242@celery-golden";
const PINNED_REPLY_TO: &str = "0f0f0f0f-0000-4000-8000-000000000000";

fn case_id(index: usize) -> Uuid {
    // `cases.py` writes the decimal index into the UUID's hex tail
    // (`f"...-{index:012d}"`), so 34 becomes `...0034`, not `...0022`.
    let text = format!("00000000-0000-4000-8000-{index:012}");
    Uuid::parse_str(&text).expect("case id is a UUID")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut url = None;
    let mut case = None;
    let mut all = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--url" => url = args.next(),
            "--case" => case = args.next(),
            "--all" => all = true,
            other => return Err(format!("unknown argument {other:?}").into()),
        }
    }
    let url = url.ok_or("--url is required")?;
    let now = DateTime::parse_from_rfc3339(PINNED_NOW)?.with_timezone(&Utc);
    let publisher = CeleryPublisher::from_url(url)?;
    publisher.connect().await?;

    let ids = test_cases::case_ids();
    let selected: Vec<(&str, usize)> = if all {
        ids.iter().enumerate().map(|(i, case)| (*case, i)).collect()
    } else {
        let case = case.ok_or("--case or --all is required")?;
        let index = ids
            .iter()
            .position(|known| *known == case)
            .ok_or_else(|| format!("unknown case {case:?}"))?;
        vec![(ids[index], index)]
    };

    for (case, index) in selected {
        let message =
            test_cases::build(case).ok_or_else(|| format!("no builder for case {case:?}"))?;
        let id = case_id(index + 1);
        let encoded = message.encode(&EncodeContext {
            id,
            now,
            origin: PINNED_ORIGIN,
            reply_to: PINNED_REPLY_TO,
            root_id: None,
        })?;
        publisher.publish_encoded(&encoded).await?;
        println!("published {case} task_id={id} bytes={}", encoded.body.len());
    }
    Ok(())
}
