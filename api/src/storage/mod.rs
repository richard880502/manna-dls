//! Where original material files live (docs/BLUEPRINT.md §13.3).
//!
//! One small trait, two backends: an S3-compatible service (MinIO, path-style) and the local
//! filesystem (development and tests). Objects are private. The browser never talks to the
//! store: every read goes through the API, which checks ownership first. Nothing here assumes
//! bucket versioning: `delete` removes the object, and a store that keeps old versions must not
//! be used (the MinIO bucket is created without versioning).

pub mod fs;
pub mod s3;

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::Stream;

pub type ByteStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

/// An inclusive byte range, as in `Range: bytes=start-end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

impl ByteRange {
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }
    pub fn is_empty(&self) -> bool {
        false
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("object not found")]
    NotFound,
    #[error("storage error: {0:#}")]
    Other(#[from] anyhow::Error),
}

#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    /// `"s3"` or `"fs"`.
    fn backend(&self) -> &'static str;
    /// Stores (or replaces) the object.
    async fn put(&self, key: &str, content_type: &str, data: Bytes) -> Result<(), StorageError>;
    /// Streams the object, or the given range of it.
    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ByteStream, StorageError>;
    /// Removes the object for good. Deleting an object that is not there is not an error.
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}

/// Keys are made by the server (`t<teacher>/<random>.<ext>`); this is the last line of defence
/// against a key that could leave the storage root.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && !key.starts_with('/')
        && !key.ends_with('/')
        && key
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'))
}

// ---------------------------------------------------------------- configuration

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    S3,
    Fs,
    /// Uploads are refused with a clear message; pasted text and links still work.
    None,
}

impl StorageBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S3 => "s3",
            Self::Fs => "fs",
            Self::None => "none",
        }
    }
}

#[derive(Clone)]
pub struct S3Settings {
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    pub access_key: String,
    pub secret_key: String,
}

/// What the environment asks for. A broken storage configuration never stops the server: it
/// becomes [`StorageBackend::None`] with the reason in `problem`.
#[derive(Clone)]
pub struct StorageConfig {
    pub backend: StorageBackend,
    pub dir: PathBuf,
    pub s3: Option<S3Settings>,
    pub problem: Option<String>,
}

// Hand-written so the keys never reach logs.
impl std::fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StorageConfig")
            .field("backend", &self.backend)
            .field("dir", &self.dir)
            .field("s3_endpoint", &self.s3.as_ref().map(|s| &s.endpoint))
            .field("s3_bucket", &self.s3.as_ref().map(|s| &s.bucket))
            .field("problem", &self.problem)
            .finish()
    }
}

pub const DEFAULT_STORAGE_DIR: &str = "./data/materials";
pub const DEFAULT_S3_REGION: &str = "us-east-1";

impl StorageConfig {
    pub fn none() -> Self {
        Self {
            backend: StorageBackend::None,
            dir: PathBuf::from(DEFAULT_STORAGE_DIR),
            s3: None,
            problem: None,
        }
    }

    pub fn fs(dir: impl Into<PathBuf>) -> Self {
        Self {
            backend: StorageBackend::Fs,
            dir: dir.into(),
            s3: None,
            problem: None,
        }
    }

    /// `STORAGE_BACKEND=s3|fs|none` (unset: `fs`), `STORAGE_DIR`, and for s3 `S3_ENDPOINT`,
    /// `S3_BUCKET`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, `S3_REGION` (optional).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Self {
        let dir = PathBuf::from(get("STORAGE_DIR").unwrap_or_else(|| DEFAULT_STORAGE_DIR.into()));
        let raw = get("STORAGE_BACKEND")
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "fs".into());
        let disabled = |problem: String| Self {
            backend: StorageBackend::None,
            dir: dir.clone(),
            s3: None,
            problem: Some(problem),
        };
        match raw.as_str() {
            "fs" => Self {
                backend: StorageBackend::Fs,
                dir,
                s3: None,
                problem: None,
            },
            "none" => Self {
                backend: StorageBackend::None,
                dir,
                s3: None,
                problem: None,
            },
            "s3" => {
                let need = |k: &'static str| get(k).map(|v| v.trim().to_string()).ok_or(k);
                let settings = (|| {
                    Ok::<_, &'static str>(S3Settings {
                        endpoint: need("S3_ENDPOINT")?,
                        bucket: need("S3_BUCKET")?,
                        access_key: need("S3_ACCESS_KEY")?,
                        secret_key: need("S3_SECRET_KEY")?,
                        region: get("S3_REGION")
                            .map(|v| v.trim().to_string())
                            .unwrap_or_else(|| DEFAULT_S3_REGION.into()),
                    })
                })();
                match settings {
                    Ok(s) if s3::S3Storage::new(&s).is_err() => disabled(format!(
                        "STORAGE_BACKEND=s3 but S3_ENDPOINT {:?} is not a valid http(s) URL",
                        s.endpoint
                    )),
                    Ok(s) => Self {
                        backend: StorageBackend::S3,
                        dir,
                        s3: Some(s),
                        problem: None,
                    },
                    Err(missing) => {
                        disabled(format!("STORAGE_BACKEND=s3 but {missing} is not set"))
                    }
                }
            }
            other => disabled(format!(
                "STORAGE_BACKEND has an unknown value {other:?} (expected s3, fs or none)"
            )),
        }
    }

    /// The backend to use, or `None` when uploads are off.
    pub fn build(&self) -> Option<Arc<dyn Storage>> {
        match self.backend {
            StorageBackend::None => None,
            StorageBackend::Fs => Some(Arc::new(fs::FsStorage::new(self.dir.clone()))),
            StorageBackend::S3 => {
                let settings = self.s3.as_ref()?;
                match s3::S3Storage::new(settings) {
                    Ok(s) => Some(Arc::new(s)),
                    Err(e) => {
                        tracing::error!(error = %e, "S3 storage could not be set up; uploads are off");
                        None
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> StorageConfig {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        StorageConfig::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn keys_cannot_leave_the_root() {
        assert!(valid_key("t12/0a1b2c.pdf"));
        for bad in [
            "",
            "/etc/passwd",
            "t1/../x",
            "t1//x",
            "t1/x/",
            "t1/a b",
            "t1\\x",
            "./x",
        ] {
            assert!(!valid_key(bad), "{bad}");
        }
    }

    #[test]
    fn defaults_to_the_filesystem() {
        let c = cfg(&[]);
        assert_eq!(c.backend, StorageBackend::Fs);
        assert_eq!(c.dir, PathBuf::from(DEFAULT_STORAGE_DIR));
        assert_eq!(
            cfg(&[("STORAGE_BACKEND", "none")]).backend,
            StorageBackend::None
        );
    }

    #[test]
    fn a_broken_s3_configuration_turns_uploads_off_instead_of_failing() {
        let c = cfg(&[
            ("STORAGE_BACKEND", "s3"),
            ("S3_ENDPOINT", "http://minio:9000"),
        ]);
        assert_eq!(c.backend, StorageBackend::None);
        assert!(c.problem.as_deref().unwrap().contains("S3_BUCKET"));
        assert!(c.build().is_none());

        let c = cfg(&[("STORAGE_BACKEND", "gcs")]);
        assert_eq!(c.backend, StorageBackend::None);
        assert!(c.problem.is_some());

        let c = cfg(&[
            ("STORAGE_BACKEND", "s3"),
            ("S3_ENDPOINT", "not a url"),
            ("S3_BUCKET", "b"),
            ("S3_ACCESS_KEY", "k"),
            ("S3_SECRET_KEY", "s"),
        ]);
        assert_eq!(c.backend, StorageBackend::None);
    }

    #[test]
    fn s3_settings_are_read_and_the_secret_is_not_printed() {
        let c = cfg(&[
            ("STORAGE_BACKEND", "S3"),
            ("S3_ENDPOINT", "http://minio:9000"),
            ("S3_BUCKET", "manna-materials"),
            ("S3_ACCESS_KEY", "access"),
            ("S3_SECRET_KEY", "very-secret"),
        ]);
        assert_eq!(c.backend, StorageBackend::S3);
        let s3 = c.s3.as_ref().unwrap();
        assert_eq!(s3.region, DEFAULT_S3_REGION);
        assert_eq!(s3.bucket, "manna-materials");
        let dbg = format!("{c:?}");
        assert!(
            !dbg.contains("very-secret") && !dbg.contains("access\""),
            "{dbg}"
        );
        assert!(c.build().is_some());
    }
}
