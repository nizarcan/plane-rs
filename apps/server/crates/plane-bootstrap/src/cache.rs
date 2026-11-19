// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `clear_cache` (`plane/db/management/commands/clear_cache.py:17-30`).
//!
//! `django.core.cache.cache.clear()` on django-redis is `DefaultClient.clear()`
//! (`django_redis/client/default.py:550-561`), which calls **`FLUSHDB`** on the
//! configured `REDIS_URL` database — not a key-prefix scan. Ported as-is: it
//! also wipes throttle buckets and magic-link codes on every API start, which is
//! upstream behaviour (`PROGRESS.md:1726`). The command's failures are logged
//! and swallowed.

use plane_cache::Redis;
use plane_core::config::Settings;

/// `FLUSHDB`. Returns the error for the caller's log/evidence; the boot step
/// never fails on it, like the command.
pub async fn clear_cache(settings: &Settings) -> Option<plane_cache::Error> {
    let redis = match Redis::from_settings(settings) {
        Ok(redis) => redis,
        Err(error) => {
            println!("Failed to clear cache");
            tracing::warn!(target: "plane.boot", %error, "clear_cache: invalid REDIS_URL");
            return Some(error);
        }
    };
    match redis.flush_db().await {
        Ok(()) => {
            println!("Cache Cleared");
            None
        }
        Err(error) => {
            println!("Failed to clear cache");
            tracing::warn!(target: "plane.boot", %error, "clear_cache: FLUSHDB failed");
            Some(error)
        }
    }
}
