// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! S3/MinIO object storage for the Plane rewrite.
//!
//! The contract is `apps/api/plane/settings/storage.py`: this crate emits the
//! same presigned POST form fields, policy document bytes, presigned GET URL
//! bytes and object metadata that boto3 1.34 (botocore 1.34.162) produces when
//! Plane configures it. `tests/golden/presign.json` holds 281 cases recorded
//! from the real Python wrapper under a frozen clock; `tests/golden.rs`
//! compares every URL, field name, field order, field value and policy byte.
//!
//! ```text
//! // shared at startup
//! let storage = plane_storage::Storage::from_process_env(&settings.storage)?;
//! // per request, like S3Storage(request=request)
//! let view = storage.request(RequestInfo { scheme: "https", host: "plane.example.com" });
//! let upload = view.presign_post(PresignPost { key, file_type: Some("image/png"), file_size, expiration: None }).await?;
//! let download = view.presign_get(PresignGet { key, expiration: None, disposition: "inline", filename: Some("pic.png"), content_type: Some("image/png") }).await?;
//! ```
//!
//! The decision record is `DECISIONS.md` D6. The behaviour that surprised while
//! porting is listed in the session report; the tests cite the Python line for
//! each quirk.

mod credentials;
mod endpoint;
pub mod error;
pub mod metadata;
mod percent;
mod policy;
mod sign;
mod storage;

pub use error::StorageError;
pub use metadata::ObjectMetadata;
pub use storage::{
    INLINE_SAFE_MIME_TYPES, PresignGet, PresignPost, PresignedPost, RequestInfo, Storage,
    StorageRequest,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// A handler will keep one `Storage` in app state and call it from many
    /// request tasks, so both auto traits are part of the API.
    #[test]
    fn storage_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Storage>();
        assert_send_sync::<StorageError>();
        assert_send_sync::<ObjectMetadata>();
    }
}
