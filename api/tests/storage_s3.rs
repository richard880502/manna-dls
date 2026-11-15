//! The S3 backend against a real S3-compatible server (MinIO). Not part of CI (CI uses the
//! filesystem backend): it runs only when `S3_TEST_ENDPOINT` is set.
//!
//! ```sh
//! # minio/minio is no longer published on Docker Hub; Coolify ships its own build.
//! docker run -d --name manna-minio-test -p 127.0.0.1:59000:9000 \
//!   -e MINIO_ROOT_USER=manna-test-access -e MINIO_ROOT_PASSWORD=manna-test-secret-key \
//!   ghcr.io/coollabsio/minio server /data
//! S3_TEST_ENDPOINT=http://127.0.0.1:59000 S3_TEST_ACCESS_KEY=manna-test-access \
//!   S3_TEST_SECRET_KEY=manna-test-secret-key cargo test --test storage_s3 -- --nocapture
//! ```
//!
//! The test creates its own bucket (`manna-test-<random>`), without versioning, and removes it.

use std::time::Duration;

use bytes::Bytes;
use futures_util::StreamExt;
use manna_api::storage::s3::S3Storage;
use manna_api::storage::{ByteRange, ByteStream, S3Settings, Storage, StorageError};
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

async fn read(mut stream: ByteStream) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        out.extend_from_slice(&chunk.expect("chunk"));
    }
    out
}

#[tokio::test]
async fn s3_backend_puts_reads_ranges_and_really_deletes() {
    let Ok(endpoint) = std::env::var("S3_TEST_ENDPOINT") else {
        eprintln!("skipping: S3_TEST_ENDPOINT is not set");
        return;
    };
    let access_key = std::env::var("S3_TEST_ACCESS_KEY").expect("S3_TEST_ACCESS_KEY");
    let secret_key = std::env::var("S3_TEST_SECRET_KEY").expect("S3_TEST_SECRET_KEY");
    let bucket_name = format!("manna-test-{}", &manna_api::auth::new_token()[..12]);
    let settings = S3Settings {
        endpoint: endpoint.clone(),
        bucket: bucket_name.clone(),
        region: "us-east-1".into(),
        access_key: access_key.clone(),
        secret_key: secret_key.clone(),
    };
    let store = S3Storage::new(&settings).expect("settings");

    // The bucket does not exist yet: the startup check says so instead of panicking.
    assert!(store.check().await.is_err());
    let http = reqwest::Client::new();
    let bucket = Bucket::new(
        endpoint.parse().unwrap(),
        UrlStyle::Path,
        bucket_name.clone(),
        "us-east-1",
    )
    .unwrap();
    let credentials = Credentials::new(access_key, secret_key);
    let sign = Duration::from_secs(60);
    let created = http
        .put(bucket.create_bucket(&credentials).sign(sign))
        .send()
        .await
        .unwrap();
    assert!(created.status().is_success(), "{}", created.status());
    store.check().await.expect("bucket reachable");

    // Objects are private: an unsigned request is refused.
    let key = "t7/0123456789abcdef.txt";
    let data: Vec<u8> = (0..200_000u32).flat_map(|n| n.to_le_bytes()).collect();
    assert!(!store.exists(key).await.unwrap());
    assert!(matches!(
        store.get(key, None).await,
        Err(StorageError::NotFound)
    ));
    store
        .put(key, "text/plain; charset=utf-8", Bytes::from(data.clone()))
        .await
        .expect("put");
    assert!(store.exists(key).await.unwrap());
    let anonymous = http
        .get(format!("{endpoint}/{bucket_name}/{key}"))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), 403, "anonymous read must be refused");

    // Whole object and a range.
    assert_eq!(read(store.get(key, None).await.unwrap()).await, data);
    let part = store
        .get(
            key,
            Some(ByteRange {
                start: 1000,
                end: 1999,
            }),
        )
        .await
        .unwrap();
    assert_eq!(read(part).await, &data[1000..2000]);

    // Overwrite, then delete: the object is gone, and deleting again is fine.
    store
        .put(key, "text/plain", Bytes::from_static(b"second"))
        .await
        .unwrap();
    assert_eq!(read(store.get(key, None).await.unwrap()).await, b"second");
    store.delete(key).await.expect("delete");
    assert!(!store.exists(key).await.unwrap());
    assert!(matches!(
        store.get(key, None).await,
        Err(StorageError::NotFound)
    ));
    store.delete(key).await.expect("deleting a missing object");
    // No version or delete marker is left behind: the bucket lists as empty.
    let listing = http
        .get(format!(
            "{}",
            bucket.list_objects_v2(Some(&credentials)).sign(sign)
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!listing.contains("<Key>"), "{listing}");
    assert!(store.delete("../escape").await.is_err());

    let removed = http
        .delete(bucket.delete_bucket(&credentials).sign(sign))
        .send()
        .await
        .unwrap();
    assert!(removed.status().is_success(), "{}", removed.status());
}
