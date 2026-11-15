//! S3-compatible backend (MinIO). Path-style addressing (`http://minio:9000/<bucket>/<key>`),
//! which is what MinIO serves without DNS setup. Requests are signed with rusty-s3 (SigV4,
//! pre-signed URLs that never leave this process) and sent with reqwest.

use std::time::Duration;

use anyhow::{anyhow, Context};
use bytes::Bytes;
use futures_util::TryStreamExt;
use reqwest::header::{CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

use super::{valid_key, ByteRange, ByteStream, S3Settings, Storage, StorageError};

/// How long a signed request is valid. Only this process uses the URL, immediately.
const SIGN_FOR: Duration = Duration::from_secs(300);

pub struct S3Storage {
    bucket: Bucket,
    credentials: Credentials,
    http: reqwest::Client,
}

impl S3Storage {
    pub fn new(settings: &S3Settings) -> anyhow::Result<Self> {
        let endpoint: reqwest::Url = settings
            .endpoint
            .parse()
            .with_context(|| format!("S3_ENDPOINT {:?} is not a URL", settings.endpoint))?;
        if !matches!(endpoint.scheme(), "http" | "https") {
            return Err(anyhow!("S3_ENDPOINT must be an http(s) URL"));
        }
        let bucket = Bucket::new(
            endpoint,
            UrlStyle::Path,
            settings.bucket.clone(),
            settings.region.clone(),
        )
        .map_err(|e| anyhow!("invalid S3 bucket settings: {e}"))?;
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .context("building the S3 HTTP client")?;
        Ok(Self {
            bucket,
            credentials: Credentials::new(settings.access_key.clone(), settings.secret_key.clone()),
            http,
        })
    }

    /// Make sure the bucket exists and answers with these credentials (run once at startup).
    /// A missing bucket is created; MinIO creates buckets without versioning, which hard delete
    /// relies on.
    pub async fn ensure_bucket(&self) -> anyhow::Result<BucketState> {
        let url = self
            .bucket
            .head_bucket(Some(&self.credentials))
            .sign(SIGN_FOR);
        let resp = self
            .http
            .head(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("reaching the S3 endpoint")?;
        if resp.status().is_success() {
            return Ok(BucketState::Existed);
        }
        if resp.status() != StatusCode::NOT_FOUND {
            return Err(anyhow!("HEAD bucket answered {}", resp.status()));
        }
        let url = self.bucket.create_bucket(&self.credentials).sign(SIGN_FOR);
        let resp = self
            .http
            .put(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("creating the S3 bucket")?;
        if resp.status().is_success() {
            Ok(BucketState::Created)
        } else {
            Err(anyhow!("creating the bucket answered {}", resp.status()))
        }
    }
}

/// What [`S3Storage::ensure_bucket`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketState {
    Existed,
    Created,
}

fn check_key(key: &str) -> Result<(), StorageError> {
    if valid_key(key) {
        Ok(())
    } else {
        Err(StorageError::Other(anyhow!("invalid storage key {key:?}")))
    }
}

async fn failure(what: &str, resp: reqwest::Response) -> StorageError {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.chars().take(300).collect();
    StorageError::Other(anyhow!("S3 {what} answered {status}: {body}"))
}

#[async_trait::async_trait]
impl Storage for S3Storage {
    fn backend(&self) -> &'static str {
        "s3"
    }

    async fn put(&self, key: &str, content_type: &str, data: Bytes) -> Result<(), StorageError> {
        check_key(key)?;
        let url = self
            .bucket
            .put_object(Some(&self.credentials), key)
            .sign(SIGN_FOR);
        let resp = self
            .http
            .put(url)
            .header(CONTENT_TYPE, content_type)
            .timeout(Duration::from_secs(600))
            .body(data)
            .send()
            .await
            .context("S3 PUT")?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(failure("PUT", resp).await)
        }
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ByteStream, StorageError> {
        check_key(key)?;
        let url = self
            .bucket
            .get_object(Some(&self.credentials), key)
            .sign(SIGN_FOR);
        let mut req = self.http.get(url);
        if let Some(r) = range {
            req = req.header(RANGE, format!("bytes={}-{}", r.start, r.end));
        }
        let resp = req.send().await.context("S3 GET")?;
        match resp.status() {
            s if s.is_success() => Ok(Box::pin(resp.bytes_stream().map_err(std::io::Error::other))),
            StatusCode::NOT_FOUND => Err(StorageError::NotFound),
            _ => Err(failure("GET", resp).await),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        check_key(key)?;
        let url = self
            .bucket
            .delete_object(Some(&self.credentials), key)
            .sign(SIGN_FOR);
        let resp = self
            .http
            .delete(url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .context("S3 DELETE")?;
        // S3 answers 204 whether or not the object existed.
        if resp.status().is_success() || resp.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(failure("DELETE", resp).await)
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        check_key(key)?;
        let url = self
            .bucket
            .head_object(Some(&self.credentials), key)
            .sign(SIGN_FOR);
        let resp = self
            .http
            .head(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .context("S3 HEAD")?;
        match resp.status() {
            s if s.is_success() => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            _ => Err(failure("HEAD", resp).await),
        }
    }
}
