// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `tracing` initialisation (DECISIONS.md D14).
//!
//! Production Django writes JSON to stdout (`plane/settings/production.py:55-67`,
//! `python-json-logger`), so JSON is the default. `PLANE_RS_LOG_FORMAT` can
//! switch to a human-readable format for local work (ENV.md §6).
//!
//! The format is read directly from the environment here, before `Settings` is
//! loaded: `Settings::from_env` itself logs the `SECRET_KEY` warnings, and they
//! must not be dropped because the subscriber is installed afterwards. An
//! invalid value is treated as `json` here and reported as a startup error by
//! `Settings::from_env`.

use plane_core::config::LogFormat;
use tracing::Level;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Install the global subscriber, choosing the format from the environment.
pub fn init_from_env() {
    let format = match std::env::var("PLANE_RS_LOG_FORMAT") {
        Ok(value) => LogFormat::parse_str(&value).unwrap_or(LogFormat::Json),
        Err(_) => LogFormat::Json,
    };
    init(format);
}

/// Install the global subscriber with an explicit format.
pub fn init(format: LogFormat) {
    // Django logs at INFO (production.py); the Rust default would otherwise
    // include `axum::serve` TRACE lines and internal DEBUG output.
    let registry = tracing_subscriber::registry().with(LevelFilter::from_level(Level::INFO));
    match format {
        LogFormat::Json => {
            // One JSON object per line, like `python-json-logger`. Spans are not
            // used yet, so the span fields stay off.
            registry
                .with(
                    fmt::layer()
                        .json()
                        .with_current_span(false)
                        .with_span_list(false),
                )
                .init();
        }
        LogFormat::Pretty => registry.with(fmt::layer().pretty()).init(),
        LogFormat::Compact => registry.with(fmt::layer().compact()).init(),
    }
}
