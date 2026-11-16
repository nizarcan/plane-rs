// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Live MinIO tests: a Rust presigned POST uploads, a Rust presigned GET reads
//! back, and head/copy/delete/put/get round-trip against the dev bucket.
//!
//! Gated on `PLANE_RS_TEST_S3_URL` (for example `http://127.0.0.1:59000`).
//! With `PLANE_RS_REQUIRE_DB_TESTS=1` the variable is required and the suite
//! fails instead of skipping, the same convention `plane-db` uses for its
//! schema checks. Objects live under `t_storage/<run-id>/` and are deleted
//! before the test ends.
//!
//! ```sh
//! cd apps/server
//! PLANE_RS_TEST_S3_URL=http://127.0.0.1:59000 \
//!   PLANE_RS_REQUIRE_DB_TESTS=1 \
//!   CARGO_TARGET_DIR=target/storage cargo test -p plane-storage --test live_minio -- --test-threads=1
//! ```

use plane_core::config::{EnvMap, Role, Settings};
use plane_storage::{PresignGet, PresignPost, RequestInfo, Storage};
use reqwest::Client;

fn env_map(value: &str) -> EnvMap {
    EnvMap::new()
        .set("SECRET_KEY", "test-secret-key")
        .set("REDIS_URL", "redis://127.0.0.1:56379/15")
        .set(
            "DATABASE_URL",
            "postgresql://plane:plane@127.0.0.1:55432/plane",
        )
        .set("DEBUG", "0")
        .set("FILE_SIZE_LIMIT", "5242880")
        .set("AWS_S3_ENDPOINT_URL", value)
        .set("AWS_S3_BUCKET_NAME", "uploads")
        .set("AWS_ACCESS_KEY_ID", "access-key")
        .set("AWS_SECRET_ACCESS_KEY", "secret-key")
        .set("AWS_REGION", "us-east-1")
}

/// Test configuration and a unique key prefix.
struct Live {
    url: String,
    storage: Storage,
    prefix: String,
}

fn live_or_skip() -> Option<Live> {
    let Ok(url) = std::env::var("PLANE_RS_TEST_S3_URL") else {
        if std::env::var("PLANE_RS_REQUIRE_DB_TESTS").as_deref() == Ok("1") {
            panic!("PLANE_RS_TEST_S3_URL is required when PLANE_RS_REQUIRE_DB_TESTS=1");
        }
        eprintln!("skipping live MinIO test: PLANE_RS_TEST_S3_URL is not set");
        return None;
    };
    let mut env = env_map(&url);
    if let Ok(value) = std::env::var("PLANE_RS_TEST_S3_BUCKET") {
        env = env.set("AWS_S3_BUCKET_NAME", &value);
    }
    if let Ok(value) = std::env::var("PLANE_RS_TEST_S3_ACCESS_KEY") {
        env = env.set("AWS_ACCESS_KEY_ID", &value);
    }
    if let Ok(value) = std::env::var("PLANE_RS_TEST_S3_SECRET_KEY") {
        env = env.set("AWS_SECRET_ACCESS_KEY", &value);
    }
    if let Ok(value) = std::env::var("PLANE_RS_TEST_S3_REGION") {
        env = env.set("AWS_REGION", &value);
    }
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    let storage = Storage::new(&settings.storage, &env).unwrap();
    let prefix = format!("t_storage/{}", uuid::Uuid::new_v4().simple());
    Some(Live {
        url,
        storage,
        prefix,
    })
}

/// The MIME type multipart helper.
fn multipart_body(
    post: &plane_storage::PresignedPost,
    filename: &str,
    content_type: &str,
    body: &[u8],
) -> (String, Vec<u8>) {
    let boundary = format!("----plane-storage-{}", uuid::Uuid::new_v4().simple());
    let mut out = Vec::new();
    for (name, value) in &post.fields {
        let value = value.as_deref().unwrap_or("");
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        out.extend_from_slice(value.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
    out.extend_from_slice(body);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (boundary, out)
}

async fn upload_via_presigned_post(
    client: &Client,
    post: &plane_storage::PresignedPost,
    filename: &str,
    content_type: &str,
    body: &[u8],
) {
    let (boundary, payload) = multipart_body(post, filename, content_type, body);
    let response = client
        .post(&post.url)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(payload)
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "presigned POST failed: {} {}",
        response.status(),
        response.text().await.unwrap_or_default()
    );
}

#[tokio::test]
async fn presigned_post_and_get_round_trip() {
    let Some(live) = live_or_skip() else {
        return;
    };
    let client = Client::new();
    let key = format!("{}/hello world.txt", live.prefix);
    let body = b"hello from plane-storage\n";
    let view = live.storage.worker();

    let post = view
        .presign_post(PresignPost {
            key: &key,
            file_type: Some("text/plain"),
            file_size: 4096,
            expiration: Some(300),
        })
        .await
        .unwrap();
    upload_via_presigned_post(&client, &post, "hello world.txt", "text/plain", body).await;

    let metadata = view
        .head_object(&key)
        .await
        .unwrap()
        .expect("head after POST");
    assert_eq!(metadata.content_type.as_deref(), Some("text/plain"));
    assert_eq!(metadata.content_length, Some(body.len() as i64));
    // A single-part PutObject ETag is the quoted MD5 of the body.
    let digest = {
        use md5::Digest as _;
        md5::Md5::digest(body)
    };
    let etag: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let quoted = format!("\"{etag}\"");
    assert_eq!(metadata.etag.as_deref(), Some(quoted.as_str()));
    assert!(metadata.metadata.is_empty());
    assert!(metadata.last_modified.is_some());

    // Not an inline-safe type: the disposition is attachment.
    let url = view
        .presign_get(PresignGet {
            key: &key,
            expiration: Some(300),
            disposition: "inline",
            filename: Some("hello world.txt"),
            content_type: Some("text/plain"),
        })
        .await
        .unwrap();
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let disposition = response
        .headers()
        .get("content-disposition")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert_eq!(response.bytes().await.unwrap().as_ref(), body);
    assert!(
        disposition.starts_with("attachment; filename*=UTF-8''hello%20world.txt"),
        "{disposition}"
    );

    // Inline for an allow-listed type, and MinIO honours the pinned response
    // Content-Type even though the stored object is text/plain.
    let url = view
        .presign_get(PresignGet {
            key: &key,
            expiration: Some(300),
            disposition: "inline",
            filename: Some("hello world.txt"),
            content_type: Some("image/png"),
        })
        .await
        .unwrap();
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );

    assert!(view.delete_objects(&[&key]).await.unwrap());
    assert!(view.head_object(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn head_copy_delete_and_put_get() {
    let Some(live) = live_or_skip() else {
        return;
    };
    let client = Client::new();
    let view = live.storage.worker();
    let source = format!("{}/source é.txt", live.prefix);
    let copy = format!("{}/copy é.txt", live.prefix);
    let body = b"copy me";

    assert!(
        view.upload_file(&source, body, Some("text/plain"))
            .await
            .unwrap()
    );
    let original = view.head_object(&source).await.unwrap().expect("uploaded");
    assert_eq!(original.content_length, Some(body.len() as i64));

    assert_eq!(view.copy_object(&source, &copy).await.unwrap(), Some(()));
    let duplicated = view.head_object(&copy).await.unwrap().expect("copied");
    assert_eq!(duplicated.etag, original.etag);
    assert_eq!(duplicated.content_length, original.content_length);
    assert_eq!(duplicated.content_type, original.content_type);

    // Rewrite the source through put_object/get_object and read it back.
    assert!(
        view.put_object(
            &source,
            b"rewritten",
            Some("application/octet-stream"),
            Some("public-read")
        )
        .await
        .is_ok()
    );
    assert_eq!(view.get_object(&source).await.unwrap(), b"rewritten");

    // A non-ASCII key: the path must be percent-encoded the way boto3 does and
    // MinIO must accept the signature for it.
    let unicode_url = view
        .presign_get(PresignGet {
            key: &source,
            expiration: Some(60),
            disposition: "attachment",
            filename: Some("source é.txt"),
            content_type: Some("text/plain"),
        })
        .await
        .unwrap();
    let response = client.get(&unicode_url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes().await.unwrap().as_ref(), b"rewritten");

    assert!(view.delete_objects(&[&source, &copy]).await.unwrap());
    assert!(view.head_object(&source).await.unwrap().is_none());
    assert!(view.head_object(&copy).await.unwrap().is_none());
    // Deleting a missing key is a 204, not an error.
    view.delete_object(&source).await.unwrap();

    // Plane's swallows: missing objects and failed uploads.
    assert!(view.copy_object(&source, &copy).await.unwrap().is_none());
    let mut env = env_map(&live.url);
    env = env.set("AWS_S3_BUCKET_NAME", "t-storage-does-not-exist");
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    let broken = Storage::new(&settings.storage, &env).unwrap();
    assert!(
        !broken
            .worker()
            .upload_file(&source, b"x", Some("text/plain"))
            .await
            .unwrap()
    );
    assert!(
        broken
            .worker()
            .head_object(&source)
            .await
            .unwrap()
            .is_none()
    );

    // Presigned GET is accepted from the dev proxy host shape too: the URL is
    // signed for whatever authority the request produced.
    let proxy_view = live.storage.request(RequestInfo {
        scheme: "http",
        host: "127.0.0.1:59000",
    });
    assert!(
        proxy_view
            .upload_file(&source, b"via request", Some("text/plain"))
            .await
            .unwrap()
    );
    let url = proxy_view
        .presign_get(PresignGet {
            key: &source,
            expiration: Some(60),
            disposition: "attachment",
            filename: Some("source.txt"),
            content_type: Some("text/plain"),
        })
        .await
        .unwrap();
    assert_eq!(client.get(&url).send().await.unwrap().status(), 200);
    assert!(proxy_view.delete_objects(&[&source]).await.unwrap());
}

#[tokio::test]
async fn empty_region_signature_is_accepted() {
    // `AWS_REGION=""` is what `settings.AWS_REGION` defaults to; boto3 signs
    // `.../<date>//s3/aws4_request` and MinIO accepts it (verified with the
    // Python client too). Also the shape the dev worker uses when the
    // operator sets AWS_REGION to an empty string.
    let Some(live) = live_or_skip() else {
        return;
    };
    let mut env = env_map(&live.url);
    env = env.set("AWS_REGION", "");
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    let storage = Storage::new(&settings.storage, &env).unwrap();
    let view = storage.worker();
    let key = format!("{}/empty-region.txt", live.prefix);
    assert!(
        view.upload_file(&key, b"empty region", Some("text/plain"))
            .await
            .unwrap()
    );
    let url = view
        .presign_get(PresignGet {
            key: &key,
            expiration: Some(60),
            disposition: "attachment",
            filename: Some("empty-region.txt"),
            content_type: Some("text/plain"),
        })
        .await
        .unwrap();
    assert!(
        url.contains("%2F%2Fs3%2Faws4_request"),
        "expected an empty region in the scope: {url}"
    );
    assert_eq!(Client::new().get(&url).send().await.unwrap().status(), 200);
    assert!(view.delete_objects(&[&key]).await.unwrap());
}

#[tokio::test]
async fn export_upload_and_download() {
    let Some(live) = live_or_skip() else {
        return;
    };
    let mut env = env_map(&live.url);
    env = env.set("USE_MINIO", "1").set("WEB_URL", &live.url);
    let settings = Settings::from_source(&env, Role::Worker).unwrap();
    assert!(settings.storage.custom_domain.is_some());
    let storage = Storage::new(&settings.storage, &env).unwrap();
    let key = format!("{}/export-example.zip", live.prefix);
    let body = b"PK\x03\x04 fake zip";
    storage.export_upload(&key, body).await.unwrap();
    let metadata = storage.worker().head_object(&key).await.unwrap().unwrap();
    assert_eq!(metadata.content_type.as_deref(), Some("application/zip"));
    assert_eq!(metadata.content_length, Some(body.len() as i64));

    let url = storage
        .export_download_url(&key, 7 * 24 * 60 * 60)
        .await
        .unwrap();
    assert!(url.starts_with(&format!("{}/uploads/", live.url)), "{url}");
    let response = Client::new().get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.bytes().await.unwrap().as_ref(), body);
    assert!(storage.worker().delete_objects(&[&key]).await.unwrap());
}
