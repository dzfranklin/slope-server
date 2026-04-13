use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::future;

use crate::cache::{AppState, fetch_or_cached};
use crate::encode::encode_slope_webp;
use crate::slope::compute_slope;
use crate::stitch::{ElevationTile, stitch_padded};
use crate::tile::TileCoord;

/// Application error type. Maps internal errors to HTTP status codes.
pub enum AppError {
    /// Tile does not exist (out of zoom range, out of bounds, or upstream 404
    /// for the center tile).
    NotFound,
    /// Upstream returned a server error.
    BadGateway(String),
    /// Unexpected internal failure.
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "tile not found").into_response(),
            AppError::BadGateway(msg) => (StatusCode::BAD_GATEWAY, msg).into_response(),
            AppError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error").into_response()
            }
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}

/// GET /slope/{z}/{x}/{y}  (y may include a .webp extension)
pub async fn slope_tile(
    State(state): State<AppState>,
    Path((z, x, y_raw)): Path<(u32, u32, String)>,
) -> Result<Response, AppError> {
    // Strip optional .webp extension — axum doesn't support mixed
    // parameter+literal segments like `{y}.webp` in a single path segment.
    let y_str = y_raw.strip_suffix(".webp").unwrap_or(&y_raw);
    let y: u32 = y_str.parse().map_err(|_| AppError::NotFound)?;

    let upstream = &state.upstream;

    // ── Validate zoom range ───────────────────────────────────────────────────
    if z < upstream.minzoom || z > upstream.maxzoom {
        return Err(AppError::NotFound);
    }

    // ── Validate geographic bounds ────────────────────────────────────────────
    let coord = TileCoord::new(z, x, y);
    if !coord.intersects_bounds(&upstream.bounds) {
        return Err(AppError::NotFound);
    }

    // ── Build 3×3 neighbor grid ───────────────────────────────────────────────
    let neighbor_grid = coord.neighbors_3x3();

    // Flatten to a Vec of (grid_row, grid_col, Option<TileCoord>) for async fetching.
    let fetch_tasks: Vec<(usize, usize, Option<TileCoord>)> = neighbor_grid
        .iter()
        .enumerate()
        .flat_map(|(r, row)| row.iter().enumerate().map(move |(c, tc)| (r, c, *tc)))
        .collect();

    // ── Fetch all 9 tiles concurrently ───────────────────────────────────────
    let futures: Vec<_> = fetch_tasks
        .iter()
        .map(|&(r, c, tc)| {
            let state = state.clone();
            async move {
                let result = match tc {
                    Some(coord) => fetch_or_cached(&state, coord).await,
                    None => Ok(None), // out-of-range neighbor → treat as missing
                };
                (r, c, result)
            }
        })
        .collect();

    let results = future::join_all(futures).await;

    // ── Assemble the tile grid, checking for errors ───────────────────────────
    let mut tiles: [[Option<ElevationTile>; 3]; 3] = Default::default();

    for (r, c, result) in results {
        match result {
            Ok(maybe_tile) => {
                tiles[r][c] = maybe_tile;
            }
            Err(e) => {
                // Any fetch error (including upstream 5xx for any tile) → 502.
                // We do NOT silently edge-replicate on server errors — that would
                // hide upstream problems while producing subtly wrong output.
                return Err(AppError::BadGateway(format!("upstream error: {e:#}")));
            }
        }
    }

    // Center tile must exist.
    if tiles[1][1].is_none() {
        return Err(AppError::NotFound);
    }

    // ── Stitch padded buffer ──────────────────────────────────────────────────
    let tile_size = upstream.tile_size;
    let padded = stitch_padded(tiles, tile_size);

    // ── Compute slope ─────────────────────────────────────────────────────────
    let slopes = compute_slope(&padded, tile_size, z, y);

    // ── Encode as lossless WebP ───────────────────────────────────────────────
    let webp_bytes = encode_slope_webp(&slopes, tile_size).map_err(AppError::Internal)?;

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        webp_bytes,
    )
        .into_response())
}

/// GET /slope
pub async fn serve_tilejson(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json((*state.output_tilejson).clone())
}

/// GET /healthz
pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// GET /demo  — MapLibre GL JS map with OSM base + slope color-relief layer
pub async fn demo(State(state): State<AppState>) -> impl IntoResponse {
    let tj = &*state.output_tilejson;
    let slope_url = tj.tiles.first().cloned().unwrap_or_default();
    let [west, south, east, north] = tj.bounds;
    let center_lat = (south + north) / 2.0;
    let center_lon = (west + east) / 2.0;
    let zoom = tj.minzoom.max(4);

    let html = include_str!("demo.html")
        .replace("__SLOPE_URL__", &slope_url)
        .replace("__CENTER_LON__", &center_lon.to_string())
        .replace("__CENTER_LAT__", &center_lat.to_string())
        .replace("__ZOOM__", &zoom.to_string())
        .replace("__MINZOOM__", &tj.minzoom.to_string())
        .replace("__MAXZOOM__", &tj.maxzoom.to_string());

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
}
