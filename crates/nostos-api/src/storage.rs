//! Object storage: local disk in development, S3/R2 in production.
//!
//! Two stores, kept apart on purpose:
//! - **originals**: the files as uploaded, with full GPS. Private; the client
//!   uploads straight into it (a presigned PUT for S3, an authenticated PUT to
//!   the API for local disk), so large RAF files never stream through the API.
//! - **media**: thumbnails and previews, public through the media domain.

use std::ops::Range;
use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::Duration;

use axum::http::Method;
use bytes::Bytes;
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::signer::Signer;
use object_store::{ObjectStore, PutPayload};

use crate::config::StorageConfig;

#[derive(Clone)]
pub struct Storage {
    originals: Arc<dyn ObjectStore>,
    media: Arc<dyn ObjectStore>,
    /// Signs upload URLs into the originals bucket (S3 only).
    signer: Option<Arc<AmazonS3>>,
    /// Absolute base of the API, for local upload URLs.
    api_url: String,
}

/// Where and how the client must send the original file.
#[derive(Debug, serde::Serialize)]
pub struct UploadTarget {
    pub url: String,
    pub method: &'static str,
    /// True when the URL is the API itself and expects the owner token.
    pub requires_owner_token: bool,
    pub expires_in_s: u64,
}

const UPLOAD_TTL: Duration = Duration::from_secs(60 * 60);

fn local(dir: &FsPath) -> Result<Arc<dyn ObjectStore>, object_store::Error> {
    std::fs::create_dir_all(dir).map_err(|e| object_store::Error::Generic {
        store: "local",
        source: Box::new(e),
    })?;
    Ok(Arc::new(LocalFileSystem::new_with_prefix(dir)?))
}

impl Storage {
    pub fn new(cfg: &StorageConfig, api_url: &str) -> Result<Self, object_store::Error> {
        Ok(match cfg {
            StorageConfig::Local { root } => Self {
                originals: local(&root.join("originals"))?,
                media: local(&root.join("media"))?,
                signer: None,
                api_url: api_url.to_string(),
            },
            StorageConfig::S3 {
                endpoint,
                originals_bucket,
                media_bucket,
                access_key_id,
                secret_access_key,
                region,
            } => {
                let bucket = |name: &str| {
                    AmazonS3Builder::new()
                        .with_endpoint(endpoint)
                        .with_bucket_name(name)
                        .with_access_key_id(access_key_id)
                        .with_secret_access_key(secret_access_key)
                        .with_region(region)
                        .build()
                        .map(Arc::new)
                };
                let originals = bucket(originals_bucket)?;
                Self {
                    originals: originals.clone(),
                    media: bucket(media_bucket)?,
                    signer: Some(originals),
                    api_url: api_url.to_string(),
                }
            }
        })
    }

    pub async fn upload_target(
        &self,
        key: &str,
        photo_id: uuid::Uuid,
    ) -> Result<UploadTarget, object_store::Error> {
        Ok(match &self.signer {
            Some(s3) => UploadTarget {
                url: s3
                    .signed_url(Method::PUT, &Path::from(key), UPLOAD_TTL)
                    .await?
                    .to_string(),
                method: "PUT",
                requires_owner_token: false,
                expires_in_s: UPLOAD_TTL.as_secs(),
            },
            None => UploadTarget {
                url: format!("{}/photos/{photo_id}/content", self.api_url),
                method: "PUT",
                requires_owner_token: true,
                expires_in_s: UPLOAD_TTL.as_secs(),
            },
        })
    }

    pub async fn put_original(&self, key: &str, data: Bytes) -> Result<(), object_store::Error> {
        self.originals
            .put(&Path::from(key), PutPayload::from(data))
            .await
            .map(|_| ())
    }

    pub async fn original_size(&self, key: &str) -> Result<u64, object_store::Error> {
        Ok(self.originals.head(&Path::from(key)).await?.size)
    }

    pub async fn get_original(&self, key: &str) -> Result<Bytes, object_store::Error> {
        self.originals.get(&Path::from(key)).await?.bytes().await
    }

    pub async fn get_original_range(
        &self,
        key: &str,
        range: Range<u64>,
    ) -> Result<Bytes, object_store::Error> {
        self.originals.get_range(&Path::from(key), range).await
    }

    pub async fn put_media(&self, key: &str, data: Bytes) -> Result<(), object_store::Error> {
        self.media
            .put(&Path::from(key), PutPayload::from(data))
            .await
            .map(|_| ())
    }

    pub async fn get_media(&self, key: &str) -> Result<Bytes, object_store::Error> {
        self.media.get(&Path::from(key)).await?.bytes().await
    }

    pub fn is_local(&self) -> bool {
        self.signer.is_none()
    }
}
