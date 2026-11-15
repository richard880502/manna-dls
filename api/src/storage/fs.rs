//! Local-filesystem backend: one file per object under `STORAGE_DIR`. For development and
//! tests; a single-host deployment can use it with a mounted volume.

use std::io::SeekFrom;
use std::path::PathBuf;

use anyhow::Context;
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio_util::io::ReaderStream;

use super::{valid_key, ByteRange, ByteStream, Storage, StorageError};

pub struct FsStorage {
    root: PathBuf,
}

impl FsStorage {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path(&self, key: &str) -> Result<PathBuf, StorageError> {
        if !valid_key(key) {
            return Err(StorageError::Other(anyhow::anyhow!(
                "invalid storage key {key:?}"
            )));
        }
        Ok(self.root.join(key))
    }
}

fn not_found_or(e: std::io::Error, what: &str) -> StorageError {
    if e.kind() == std::io::ErrorKind::NotFound {
        StorageError::NotFound
    } else {
        StorageError::Other(anyhow::Error::new(e).context(what.to_string()))
    }
}

#[async_trait::async_trait]
impl Storage for FsStorage {
    fn backend(&self) -> &'static str {
        "fs"
    }

    async fn put(&self, key: &str, _content_type: &str, data: Bytes) -> Result<(), StorageError> {
        let path = self.path(key)?;
        if let Some(dir) = path.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .with_context(|| format!("creating {}", dir.display()))?;
        }
        // Written beside the target and renamed, so a reader never sees half a file.
        let tmp = path.with_extension("uploading");
        let mut file = tokio::fs::File::create(&tmp)
            .await
            .with_context(|| format!("creating {}", tmp.display()))?;
        let written = async {
            file.write_all(&data).await?;
            file.sync_all().await
        }
        .await;
        if let Err(e) = written {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(StorageError::Other(
                anyhow::Error::new(e).context("writing the object"),
            ));
        }
        tokio::fs::rename(&tmp, &path)
            .await
            .context("moving the object into place")?;
        Ok(())
    }

    async fn get(&self, key: &str, range: Option<ByteRange>) -> Result<ByteStream, StorageError> {
        let path = self.path(key)?;
        let mut file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| not_found_or(e, "opening the object"))?;
        match range {
            None => Ok(Box::pin(ReaderStream::new(file))),
            Some(r) => {
                file.seek(SeekFrom::Start(r.start))
                    .await
                    .context("seeking in the object")?;
                Ok(Box::pin(ReaderStream::new(file.take(r.len()))))
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let path = self.path(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Other(
                anyhow::Error::new(e).context("deleting the object"),
            )),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, StorageError> {
        let path = self.path(key)?;
        Ok(tokio::fs::try_exists(&path)
            .await
            .context("checking the object")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;

    async fn read(mut s: ByteStream) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(chunk) = s.next().await {
            out.extend_from_slice(&chunk.unwrap());
        }
        out
    }

    #[tokio::test]
    async fn put_get_range_delete() {
        let dir = std::env::temp_dir().join(format!("manna-fs-test-{}", crate::auth::new_token()));
        let store = FsStorage::new(dir.clone());
        let key = "t1/abc.txt";
        assert!(!store.exists(key).await.unwrap());
        assert!(matches!(
            store.get(key, None).await,
            Err(StorageError::NotFound)
        ));
        store
            .put(key, "text/plain", Bytes::from_static(b"0123456789"))
            .await
            .unwrap();
        assert!(store.exists(key).await.unwrap());
        assert_eq!(
            read(store.get(key, None).await.unwrap()).await,
            b"0123456789"
        );
        let part = store
            .get(key, Some(ByteRange { start: 2, end: 5 }))
            .await
            .unwrap();
        assert_eq!(read(part).await, b"2345");
        store.delete(key).await.unwrap();
        assert!(!store.exists(key).await.unwrap());
        // Deleting again is fine; an invalid key is refused before touching the disk.
        store.delete(key).await.unwrap();
        assert!(store.delete("../outside").await.is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
