use serde::{Deserialize, Serialize};

/// Raw serde target for the upstream TileJSON response (TileJSON 2.x).
#[derive(Debug, Deserialize)]
pub struct TileJsonResponse {
    pub tiles: Vec<String>,
    /// Tile pixel size. Absent in many TileJSON responses — default to 512
    /// to match MapLibre GL JS behavior (the TileJSON spec says 256, but
    /// MapLibre overrides to 512 for raster-dem sources).
    #[serde(rename = "tileSize")]
    pub tile_size: Option<u32>,
    /// "mapbox" or "terrarium". Absent → Mapbox.
    pub encoding: Option<String>,
    pub minzoom: Option<u32>,
    pub maxzoom: Option<u32>,
    /// [west, south, east, north] in WGS84 degrees.
    pub bounds: Option<[f64; 4]>,
    pub attribution: Option<String>,
}

/// The TileJSON document we serve at GET /slope.
/// Reflects hardcoded output spec (Mapbox encoding, 512px, WebP lossless)
/// with bounds/zoom range forwarded from the upstream.
#[derive(Debug, Clone, Serialize)]
pub struct OutputTileJson {
    pub tilejson: &'static str,
    pub tiles: Vec<String>,
    /// Always 512 — matches our hardcoded output tile size.
    #[serde(rename = "tileSize")]
    pub tile_size: u32,
    /// Always "mapbox" — our output uses Mapbox Terrain-RGB encoding.
    pub encoding: &'static str,
    pub minzoom: u32,
    pub maxzoom: u32,
    pub bounds: [f64; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

impl OutputTileJson {
    pub fn new(
        tile_url: String,
        minzoom: u32,
        maxzoom: u32,
        bounds: [f64; 4],
        attribution: Option<String>,
    ) -> Self {
        Self {
            tilejson: "2.2.0",
            tiles: vec![tile_url],
            tile_size: 512,
            encoding: "mapbox",
            minzoom,
            maxzoom,
            bounds,
            attribution,
        }
    }
}
