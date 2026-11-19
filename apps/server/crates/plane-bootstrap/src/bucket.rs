// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `create_bucket` (`plane/db/management/commands/create_bucket.py:14-61`).
//!
//! boto3's `head_bucket` then, when the bucket is missing, `create_bucket`.
//! Every error is swallowed and printed: the command exits 0 whatever happens,
//! so a broken object store never stops the API from starting. The branches are
//! Python's:
//!
//! * exists -> `Bucket '<name>' exists.`
//! * 404 -> `Bucket '<name>' does not exist. Creating bucket...`, then
//!   `created successfully` or `Failed to create bucket: <error>`;
//! * 403 -> `Access to the bucket '<name>' is forbidden. Check permissions.`;
//! * any other S3 error -> `Failed to check bucket: <error>`;
//! * anything else (missing bucket name, transport error) ->
//!   `An error occurred: <error>`.
//!
//! Python's `int(e.response["Error"]["Code"])` raises `ValueError` for a
//! non-numeric code (an S3 `<Code>AccessDenied</Code>` answer), which escapes
//! the command and fails it; the port branches on the HTTP status instead,
//! which is the same 404/403 split botocore synthesises for `HEAD`.

use plane_core::config::Settings;
use plane_storage::{Storage, StorageError};

/// Run the boot step. Errors are logged; the return value is only for tests and
/// the comparison harness.
pub async fn create_bucket(settings: &Settings) -> Option<StorageError> {
    let bucket = settings
        .storage
        .bucket_name_storage
        .clone()
        .unwrap_or_default();
    let storage = match Storage::from_process_env(&settings.storage) {
        Ok(storage) => storage,
        Err(error) => {
            println!("An error occurred: {error}");
            tracing::warn!(target: "plane.boot", %error, "create_bucket: storage unavailable");
            return Some(error);
        }
    };
    println!("Checking bucket...");
    match storage.head_bucket().await {
        Ok(()) => {
            println!("Bucket '{bucket}' exists.");
            None
        }
        Err(error) => {
            let status = match &error {
                StorageError::S3 { status, .. } => Some(*status),
                _ => None,
            };
            match status {
                Some(404) => {
                    println!("Bucket '{bucket}' does not exist. Creating bucket...");
                    match storage.create_bucket().await {
                        Ok(()) => {
                            println!("Bucket '{bucket}' created successfully.");
                            None
                        }
                        Err(error) => {
                            println!("Failed to create bucket: {error}");
                            tracing::warn!(target: "plane.boot", %error, "create_bucket: create failed");
                            Some(error)
                        }
                    }
                }
                Some(403) => {
                    println!("Access to the bucket '{bucket}' is forbidden. Check permissions.");
                    None
                }
                Some(_) => {
                    println!("Failed to check bucket: {error}");
                    tracing::warn!(target: "plane.boot", %error, "create_bucket: head failed");
                    Some(error)
                }
                None => {
                    println!("An error occurred: {error}");
                    tracing::warn!(target: "plane.boot", %error, "create_bucket: head failed");
                    Some(error)
                }
            }
        }
    }
}
