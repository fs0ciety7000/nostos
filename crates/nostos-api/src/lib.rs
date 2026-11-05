//! Nostos API: trips, photo uploads, background ingestion and stop
//! detection on PostgreSQL/PostGIS. See `routes.rs` for the endpoints.

pub mod config;
pub mod error;
pub mod imaging;
pub mod ingest;
pub mod routes;
pub mod stops;
pub mod storage;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, patch, post, put};
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

pub use config::Config;
use storage::Storage;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub storage: Storage,
    pub owner_token: Arc<str>,
    pub media_base_url: Arc<str>,
    /// Process uploads inside the `complete` request instead of in the
    /// background workers. Used by tests to stay deterministic.
    pub inline_ingest: bool,
}

impl AppState {
    pub async fn connect(cfg: &Config) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let db = PgPoolOptions::new()
            .max_connections(10)
            .connect(&cfg.database_url)
            .await?;
        MIGRATOR.run(&db).await?;
        Ok(Self {
            db,
            storage: Storage::new(&cfg.storage, &cfg.public_api_url)?,
            owner_token: cfg.owner_token.as_str().into(),
            media_base_url: cfg.media_base_url.as_str().into(),
            inline_ingest: false,
        })
    }

    pub fn media_url(&self, key: &str) -> String {
        format!("{}/{key}", self.media_base_url)
    }
}

pub fn app(state: AppState) -> Router {
    let owner = Router::new()
        .route("/trips", get(routes::list_trips).post(routes::create_trip))
        .route("/trips/{slug}", get(routes::get_trip))
        .route("/trips/{slug}/photos", get(routes::trip_photos))
        .route("/trips/{slug}/route", get(routes::trip_route))
        .route(
            "/trips/{slug}/stops/recompute",
            post(routes::recompute_stops),
        )
        .route("/trips/{slug}/uploads", post(routes::create_upload))
        .route("/photos/{id}", get(routes::get_photo))
        .route(
            "/photos/{id}/content",
            put(routes::put_content)
                .layer(DefaultBodyLimit::max(routes::MAX_UPLOAD_BYTES as usize)),
        )
        .route("/photos/{id}/complete", post(routes::complete_upload))
        .route("/stops/{id}", patch(routes::patch_stop))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            routes::require_owner,
        ));

    Router::new()
        .route("/health", get(routes::health))
        .route("/media/{*key}", get(routes::media))
        .merge(owner)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
