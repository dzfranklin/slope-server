use anyhow::Result;
use axum::{Router, routing::get};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use slope_server::{
    cache::AppState,
    config::{bind_addr, cache_max_tiles, cache_ttl_secs, load_upstream_config},
    handlers::{demo, healthz, serve_tilejson, slope_tile},
    tilejson::OutputTileJson,
};

const MIN_MINZOOM: u32 = 9;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let client = reqwest::Client::builder()
        .user_agent("slope-server/0.1")
        .build()?;

    let upstream = load_upstream_config(&client).await?;
    tracing::info!(
        template = %upstream.tile_template,
        encoding = ?upstream.encoding,
        tile_size = upstream.tile_size,
        minzoom = upstream.minzoom,
        maxzoom = upstream.maxzoom,
        "upstream configured"
    );

    let addr = bind_addr();
    let max_tiles = cache_max_tiles();
    let ttl_secs = cache_ttl_secs();

    // Build the output TileJSON. The tile URL is derived from BIND_ADDR at
    // runtime — in production, we rewrite the host, so we use a relative
    // path placeholder. Operators can override by setting OUTPUT_TILE_URL_BASE.
    let tile_url_base =
        std::env::var("OUTPUT_TILE_URL_BASE").unwrap_or_else(|_| format!("http://{addr}/slope"));
    let tile_url = format!("{tile_url_base}/{{z}}/{{x}}/{{y}}");

    let output_tilejson = OutputTileJson::new(
        tile_url,
        upstream.minzoom.max(MIN_MINZOOM),
        upstream.maxzoom,
        upstream.bounds,
        upstream.attribution.clone(),
    );

    let state = AppState::new(upstream, client, output_tilejson, max_tiles, ttl_secs);

    let app = Router::new()
        .route("/slope/{z}/{x}/{y}", get(slope_tile))
        .route("/slope", get(serve_tilejson))
        .route("/healthz", get(healthz))
        .route("/demo", get(demo))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(addr = %addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("server shut down");
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let sigterm = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }

    tracing::info!("shutdown signal received");
}
