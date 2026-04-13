use crate::decode::DemEncoding;
use crate::tilejson::TileJsonResponse;
use anyhow::{Context, Result, anyhow, bail};

const DEFAULT_UPSTREAM_TILEJSON: &str = "https://tiles.mapterhorn.com/tilejson.json";

/// Configuration derived from the upstream TileJSON at startup.
#[derive(Debug, Clone)]
pub struct UpstreamConfig {
    /// TileJSON URL template, e.g. "https://…/{z}/{x}/{y}.png"
    pub tile_template: String,
    /// Source tile pixel size. Default 512 — see tileSize note in TileJsonResponse.
    pub tile_size: u32,
    pub encoding: DemEncoding,
    pub minzoom: u32,
    pub maxzoom: u32,
    /// [west, south, east, north] in WGS84 degrees.
    pub bounds: [f64; 4],
    pub attribution: Option<String>,
}

/// Parse and validate environment variables and fetch the upstream TileJSON.
/// Fails fast if UPSTREAM_TILEJSON is missing, unreachable, or malformed.
pub async fn load_upstream_config(client: &reqwest::Client) -> Result<UpstreamConfig> {
    let tilejson_url =
        std::env::var("UPSTREAM_TILEJSON").unwrap_or(DEFAULT_UPSTREAM_TILEJSON.to_string());

    tracing::info!(url = %tilejson_url, "fetching upstream TileJSON");

    let response = client
        .get(&tilejson_url)
        .send()
        .await
        .with_context(|| format!("failed to fetch TileJSON from {tilejson_url}"))?;

    if !response.status().is_success() {
        bail!("upstream TileJSON fetch failed: HTTP {}", response.status());
    }

    let tj: TileJsonResponse = response
        .json()
        .await
        .with_context(|| format!("failed to parse TileJSON from {tilejson_url}"))?;

    let tile_template = tj
        .tiles
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("upstream TileJSON has no tiles URLs"))?;

    let tile_size = tj.tile_size.unwrap_or(512); // MapLibre GL JS default, not TileJSON spec default
    if tile_size != 512 {
        bail!("upstream tile_size is {tile_size}, but only 512px sources are supported");
    }

    let encoding = tj
        .encoding
        .as_deref()
        .and_then(DemEncoding::from_str)
        .unwrap_or(DemEncoding::Mapbox);

    let minzoom = tj.minzoom.unwrap_or(0);
    let maxzoom = tj.maxzoom.unwrap_or(22);

    let bounds = tj.bounds.unwrap_or([-180.0, -90.0, 180.0, 90.0]);

    Ok(UpstreamConfig {
        tile_template,
        tile_size,
        encoding,
        minzoom,
        maxzoom,
        bounds,
        attribution: tj.attribution,
    })
}

/// Parse BIND_ADDR from environment, defaulting to "0.0.0.0:8080".
pub fn bind_addr() -> String {
    std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string())
}

/// Parse CACHE_MAX_TILES from environment, defaulting to 1024.
pub fn cache_max_tiles() -> u64 {
    std::env::var("CACHE_MAX_TILES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1024)
}

/// Parse CACHE_TTL_SECS from environment, defaulting to 3600.
pub fn cache_ttl_secs() -> u64 {
    std::env::var("CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600)
}
