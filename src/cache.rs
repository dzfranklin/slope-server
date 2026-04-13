use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use moka::future::Cache;

use crate::config::UpstreamConfig;
use crate::decode::decode_tile;
use crate::fetch::{fetch_tile, FetchResult};
use crate::stitch::ElevationTile;
use crate::tile::TileCoord;
use crate::tilejson::OutputTileJson;

/// Shared application state passed to every axum handler.
#[derive(Clone)]
pub struct AppState {
    pub upstream: Arc<UpstreamConfig>,
    pub client: reqwest::Client,
    /// LRU cache of decoded source tiles, keyed by TileCoord.
    /// Individual source tiles are cached (not stitched or slope output) to
    /// maximise reuse across adjacent tile requests (8 of 9 source tiles
    /// overlap between horizontally adjacent requests).
    pub cache: Cache<TileCoord, ElevationTile>,
    pub output_tilejson: Arc<OutputTileJson>,
}

impl AppState {
    pub fn new(
        upstream: UpstreamConfig,
        client: reqwest::Client,
        output_tilejson: OutputTileJson,
        max_tiles: u64,
        ttl_secs: u64,
    ) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_tiles)
            .time_to_live(Duration::from_secs(ttl_secs))
            .build();

        Self {
            upstream: Arc::new(upstream),
            client,
            cache,
            output_tilejson: Arc::new(output_tilejson),
        }
    }
}

/// Fetch a source tile, returning the decoded elevation grid.
///
/// Returns `Ok(Some(...))` on success, `Ok(None)` if the tile is genuinely
/// absent (HTTP 404), or `Err` on network/server errors or decode failure.
///
/// Results are cached by `TileCoord`. 404s and errors are NOT cached — a
/// transient upstream failure should be retried on the next request.
///
/// Moka's async cache deduplicates concurrent fetches for the same key, so
/// concurrent requests for the same tile coord result in a single upstream
/// fetch.
pub async fn fetch_or_cached(state: &AppState, coord: TileCoord) -> Result<Option<ElevationTile>> {
    // Fast path: cache hit.
    if let Some(tile) = state.cache.get(&coord).await {
        return Ok(Some(tile));
    }

    // Slow path: fetch from upstream.
    let result = fetch_tile(&state.client, &state.upstream.tile_template, coord).await?;

    match result {
        FetchResult::NotFound => Ok(None),
        FetchResult::ServerError(status) => {
            Err(anyhow!("upstream returned HTTP {status} for tile {coord:?}"))
        }
        FetchResult::Bytes(bytes) => {
            let elevations = Arc::new(decode_tile(&bytes, state.upstream.encoding)?);
            state.cache.insert(coord, Arc::clone(&elevations)).await;
            Ok(Some(elevations))
        }
    }
}
