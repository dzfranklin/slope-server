use crate::tile::TileCoord;
use anyhow::Result;
use bytes::Bytes;

/// Result of fetching a single tile from the upstream DEM source.
pub enum FetchResult {
    /// Tile data returned successfully.
    Bytes(Bytes),
    /// Upstream returned HTTP 404 — tile does not exist (world edge, out of
    /// bounds, etc.).
    NotFound,
    /// Upstream returned a server error (5xx). Callers should propagate this
    /// as a 502 Bad Gateway rather than silently edge-replicating.
    ServerError(u16),
}

/// Expand a TileJSON URL template and fetch the tile from upstream.
///
/// The template uses `{z}`, `{x}`, `{y}` placeholders (standard TileJSON).
/// Returns `FetchResult::NotFound` for HTTP 404, `FetchResult::ServerError`
/// for 5xx, and `Err` for network/transport failures.
pub async fn fetch_tile(
    client: &reqwest::Client,
    template: &str,
    coord: TileCoord,
) -> Result<FetchResult> {
    let url = template
        .replace("{z}", &coord.z.to_string())
        .replace("{x}", &coord.x.to_string())
        .replace("{y}", &coord.y.to_string());

    let response = client.get(&url).send().await?;
    let status = response.status();
    tracing::debug!(url = %url, status = %status, "upstream response");

    if status.is_success() {
        let bytes = response.bytes().await?;
        Ok(FetchResult::Bytes(bytes))
    } else if status == reqwest::StatusCode::NOT_FOUND {
        Ok(FetchResult::NotFound)
    } else if status.is_server_error() {
        Ok(FetchResult::ServerError(status.as_u16()))
    } else {
        // Other 4xx (e.g. 401, 403) — treat as not found to avoid blocking.
        tracing::warn!(url = %url, status = %status, "unexpected upstream status");
        Ok(FetchResult::NotFound)
    }
}
