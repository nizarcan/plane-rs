// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Drain a Celery queue with `plane-testkit`'s decoder and print the
//! deliveries as JSON.
//!
//! ```sh
//! cargo run --manifest-path apps/server/tools/celery/drain/Cargo.toml -- \
//!     amqp://plane:plane@127.0.0.1:55672/t_celery_live [queue]
//! ```

use std::error::Error;

use plane_testkit::celery::CeleryDrain;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let url = args
        .next()
        .ok_or("usage: celery-drain <amqp-url> [queue]")?;
    let queue = args.next().unwrap_or_else(|| "celery".to_string());
    let drain = CeleryDrain::connect(&url).await?;
    let messages = drain.drain(&[queue]).await?;
    println!("{}", serde_json::to_string_pretty(&messages)?);
    Ok(())
}
