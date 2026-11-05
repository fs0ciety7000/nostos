use nostos_api::{app, ingest, AppState, Config};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn")),
        )
        .init();

    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}. See .env.example for the variables nostos-api reads.");
            std::process::exit(2);
        }
    };
    let state = match AppState::connect(&cfg).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("startup failed: {e}");
            std::process::exit(1);
        }
    };
    ingest::spawn_workers(&state, cfg.workers);

    let listener = tokio::net::TcpListener::bind(cfg.bind)
        .await
        .expect("bind address");
    tracing::info!(addr = %cfg.bind, storage = if state.storage.is_local() { "local" } else { "s3" }, "nostos-api listening");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("server");
}
