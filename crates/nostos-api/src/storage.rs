//! Object storage: local disk in development, S3/R2 in production.
//!
//! Originals are uploaded by the client straight to storage (a presigned
//! PUT for S3, an authenticated PUT to the API for local disk), so large RAF
//! files never stream through the API in production.

use std::ops::Range;
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
    store: Arc<dyn ObjectStore>,
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

impl Storage {
    pub fn new(cfg: &StorageConfig, api_url: &str) -> Result<Self, object_store::Error> {
        Ok(match cfg {
            StorageConfig::Local { root } => {
                std::fs::create_dir_all(root).map_err(|e| object_store::Error::Generic {
                    store: "local",
                    source: Box::new(e),
                })?;
                Self {
                    store: Arc::new(LocalFileSystem::new_with_prefix(root)?),
                    signer: None,
                    api_url: api_url.to_string(),
                }
            }
            StorageConfig::S3 {
                endpoint,
                bucket,
                access_key_id,
                secret_access_key,
                region,
            } => {
                let s3 = Arc::new(
                    AmazonS3Builder::new()
                        .with_endpoint(endpoint)
                        .with_bucket_name(bucket)
                        .with_access_key_id(access_key_id)
                        .with_secret_access_key(secret_access_key)
                        .with_region(region)
                        .build()?,
                );
                Self {
                    store: s3.clone(),
                    signer: Some(s3),
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

    pub async fn put(&self, key: &str, data: Bytes) -> Result<(), object_store::Error> {
        self.store
            .put(&Path::from(key), PutPayload::from(data))
            .await
            .map(|_| ())
    }

    pub async fn size(&self, key: &str) -> Result<u64, object_store::Error> {
        Ok(self.store.head(&Path::from(key)).await?.size)
    }

    pub async fn get(&self, key: &str) -> Result<Bytes, object_store::Error> {
        self.store.get(&Path::from(key)).await?.bytes().await
    }

    pub async fn get_range(
        &self,
        key: &str,
        range: Range<u64>,
    ) -> Result<Bytes, object_store::Error> {
        self.store.get_range(&Path::from(key), range).await
    }

    pub fn is_local(&self) -> bool {
        self.signer.is_none()
    }
}
