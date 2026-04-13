use anyhow::{bail, Context, Result};
use image::GenericImageView;

/// Encoding scheme used by the upstream DEM tile source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemEncoding {
    /// Mapbox Terrain-RGB: height = -10000 + (R*65536 + G*256 + B) * 0.1
    Mapbox,
    /// Terrarium: height = R*256 + G - 32768 + B/256
    Terrarium,
}

impl DemEncoding {
    /// Parse from TileJSON `encoding` field. Anything other than "terrarium"
    /// is treated as Mapbox (the more common default).
    pub fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("terrarium") {
            DemEncoding::Terrarium
        } else {
            DemEncoding::Mapbox
        }
    }
}

/// Decode Mapbox Terrain-RGB pixel to metres.
#[inline]
pub fn mapbox_pixel(r: u8, g: u8, b: u8) -> f32 {
    -10_000.0 + (r as f32 * 65_536.0 + g as f32 * 256.0 + b as f32) * 0.1
}

/// Decode Terrarium pixel to metres.
#[inline]
pub fn terrarium_pixel(r: u8, g: u8, b: u8) -> f32 {
    r as f32 * 256.0 + g as f32 - 32_768.0 + b as f32 / 256.0
}

/// Decode a PNG tile (as raw bytes) into a flat row-major Vec<f32> of
/// elevations in metres. Length = tile_size * tile_size.
pub fn decode_tile(bytes: &[u8], encoding: DemEncoding) -> Result<Vec<f32>> {
    let img = image::load_from_memory(bytes).context("PNG decode failed")?;
    let (width, height) = img.dimensions();

    let rgba = img.into_rgba8();
    let mut elevations = Vec::with_capacity((width * height) as usize);

    for pixel in rgba.pixels() {
        let [r, g, b, _a] = pixel.0;
        let elev = match encoding {
            DemEncoding::Mapbox => mapbox_pixel(r, g, b),
            DemEncoding::Terrarium => terrarium_pixel(r, g, b),
        };
        elevations.push(elev);
    }

    if elevations.len() != (width * height) as usize {
        bail!(
            "elevation count mismatch: got {}, expected {}",
            elevations.len(),
            width * height
        );
    }

    Ok(elevations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapbox_zero_elevation() {
        // height = -10000 + (1*65536 + 134*256 + 160) * 0.1
        // packed for 0m: round((0 + 10000) / 0.1) = 100000
        // R=1, G=134, B=160 — wait: 100000 = 0x0186A0 → R=1, G=0x86=134, B=0xA0=160
        let h = mapbox_pixel(1, 134, 160);
        assert!((h - 0.0).abs() < 0.1, "expected ~0m, got {h}");
    }

    #[test]
    fn mapbox_negative_elevation() {
        // height = -10000 + 0 * 0.1 = -10000 (ocean floor)
        let h = mapbox_pixel(0, 0, 0);
        assert!((h - (-10_000.0)).abs() < 0.01, "got {h}");
    }

    #[test]
    fn terrarium_sea_level() {
        // R=128, G=0, B=0 → 128*256 + 0 - 32768 + 0 = 32768 - 32768 = 0
        let h = terrarium_pixel(128, 0, 0);
        assert!((h - 0.0).abs() < 0.01, "got {h}");
    }

    #[test]
    fn encoding_from_str() {
        assert_eq!(DemEncoding::from_str("terrarium"), DemEncoding::Terrarium);
        assert_eq!(DemEncoding::from_str("Terrarium"), DemEncoding::Terrarium);
        assert_eq!(DemEncoding::from_str("mapbox"), DemEncoding::Mapbox);
        assert_eq!(DemEncoding::from_str(""), DemEncoding::Mapbox);
    }
}
