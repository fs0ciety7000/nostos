//! Configuration from environment variables (names in `.env.example`).

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum StorageConfig {
    /// Files on local disk, served by the API itself. For development and tests.
    Local { root: PathBuf },
    /// Any S3-compatible bucket; Cloudflare R2 in production.
    S3 {
        endpoint: String,
        bucket: String,
        access_key_id: String,
        secret_access_key: String,
        region: String,
    },
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind: SocketAddr,
    /// Bearer token required on every route except health and media.
    pub owner_token: String,
    pub storage: StorageConfig,
    /// Base URL thumbnails and previews are served from: the API itself for
    /// local storage (`http://localhost:8080/media`), the R2 public bucket
    /// or CDN domain in production.
    pub media_base_url: String,
    /// Absolute base URL of this API, used to build local upload URLs.
    pub public_api_url: String,
    pub workers: usize,
}

#[derive(Debug, thiserror::Error)]
#[error("missing or invalid environment variable {0}")]
pub struct ConfigError(pub &'static str);

fn var(name: &'static str) -> Result<String, ConfigError> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .ok_or(ConfigError(name))
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind: SocketAddr = std::env::var("NOSTOS_BIND")
            .unwrap_or_else(|_| "0.0.0.0:8080".into())
            .parse()
            .map_err(|_| ConfigError("NOSTOS_BIND"))?;
        let public_api_url = std::env::var("NOSTOS_PUBLIC_URL")
            .unwrap_or_else(|_| format!("http://localhost:{}", bind.port()));
        let storage = match std::env::var("NOSTOS_STORAGE")
            .as_deref()
            .unwrap_or("local")
        {
            "local" => StorageConfig::Local {
                root: std::env::var("NOSTOS_STORAGE_DIR")
                    .unwrap_or_else(|_| "./data/media".into())
                    .into(),
            },
            "s3" => StorageConfig::S3 {
                endpoint: var("S3_ENDPOINT")?,
                bucket: var("S3_BUCKET")?,
                access_key_id: var("S3_ACCESS_KEY_ID")?,
                secret_access_key: var("S3_SECRET_ACCESS_KEY")?,
                region: std::env::var("S3_REGION").unwrap_or_else(|_| "auto".into()),
            },
            _ => return Err(ConfigError("NOSTOS_STORAGE")),
        };
        let owner_token = var("NOSTOS_OWNER_TOKEN")?;
        if owner_token.len() < 32 {
            return Err(ConfigError("NOSTOS_OWNER_TOKEN (at least 32 characters)"));
        }
        let media_base_url = match &storage {
            StorageConfig::Local { .. } => std::env::var("NOSTOS_MEDIA_URL")
                .unwrap_or_else(|_| format!("{public_api_url}/media")),
            StorageConfig::S3 { .. } => var("NOSTOS_MEDIA_URL")?,
        };
        Ok(Self {
            database_url: var("DATABASE_URL")?,
            bind,
            owner_token,
            storage,
            media_base_url: media_base_url.trim_end_matches('/').to_string(),
            public_api_url: public_api_url.trim_end_matches('/').to_string(),
            workers: std::env::var("NOSTOS_WORKERS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2)
                .clamp(1, 16),
        })
    }
}
