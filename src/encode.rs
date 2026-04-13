use anyhow::{Context, Result};
use image::{codecs::webp::WebPEncoder, ColorType, ImageEncoder};

/// Pack slope in degrees into Mapbox Terrain-RGB bytes.
///
/// Mapbox encoding: packed = round((height + 10000) / 0.1)
///   R = (packed >> 16) & 0xFF
///   G = (packed >>  8) & 0xFF
///   B =  packed        & 0xFF
///
/// Slope is clamped to [0, 90] before packing.
pub fn slope_to_terrain_rgb(slope_deg: f32) -> [u8; 3] {
    let clamped = slope_deg.clamp(0.0, 90.0);
    // packed = round((slope + 10000) / 0.1)
    let packed = ((clamped + 10_000.0) / 0.1).round() as u32;
    let r = ((packed >> 16) & 0xFF) as u8;
    let g = ((packed >> 8) & 0xFF) as u8;
    let b = (packed & 0xFF) as u8;
    [r, g, b]
}

/// Decode Mapbox Terrain-RGB bytes back to a height value (for round-trip tests).
pub fn terrain_rgb_to_height(r: u8, g: u8, b: u8) -> f32 {
    -10_000.0 + (r as f32 * 65_536.0 + g as f32 * 256.0 + b as f32) * 0.1
}

/// Encode a Vec<f32> of slope-degrees as a lossless WebP byte stream.
///
/// Each slope value is packed into Mapbox Terrain-RGB (R, G, B) with alpha=255.
/// Output tile is always `tile_size × tile_size` pixels.
/// Lossy WebP would corrupt the encoding since adjacent elevation values can
/// have wildly different byte patterns — lossless is required.
pub fn encode_slope_webp(slopes: &[f32], tile_size: u32) -> Result<Vec<u8>> {
    let n = tile_size as usize;
    assert_eq!(
        slopes.len(),
        n * n,
        "slopes length {} != tile_size² {}",
        slopes.len(),
        n * n
    );

    // Build RGBA buffer: R, G, B from terrain-rgb packing; A = 255.
    let mut rgba = Vec::with_capacity(n * n * 4);
    for &slope in slopes {
        let [r, g, b] = slope_to_terrain_rgb(slope);
        rgba.extend_from_slice(&[r, g, b, 255]);
    }

    // Encode as lossless WebP using the image crate's WebP encoder.
    let mut out = Vec::new();
    let encoder = WebPEncoder::new_lossless(&mut out);
    encoder
        .write_image(&rgba, tile_size, tile_size, ColorType::Rgba8.into())
        .context("WebP encode failed")?;

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_slope_zero() {
        // slope=0°: packed = round(10000 / 0.1) = 100000 = 0x0186A0
        // R=1, G=0x86=134, B=0xA0=160
        let [r, g, b] = slope_to_terrain_rgb(0.0);
        let decoded = terrain_rgb_to_height(r, g, b);
        // Decoded height should be ~0 (since we stored slope=0 as height=0).
        assert!(
            decoded.abs() < 0.1,
            "round-trip 0°: decoded={decoded}"
        );
    }

    #[test]
    fn round_trip_slope_30() {
        let slope = 30.0f32;
        let [r, g, b] = slope_to_terrain_rgb(slope);
        let decoded = terrain_rgb_to_height(r, g, b);
        // Decoded height ≈ slope (since we store slope as height in the encoding).
        assert!(
            (decoded - slope).abs() < 0.1,
            "round-trip 30°: decoded={decoded}"
        );
    }

    #[test]
    fn round_trip_slope_45() {
        let slope = 45.0f32;
        let [r, g, b] = slope_to_terrain_rgb(slope);
        let decoded = terrain_rgb_to_height(r, g, b);
        assert!(
            (decoded - slope).abs() < 0.1,
            "round-trip 45°: decoded={decoded}"
        );
    }

    #[test]
    fn clamp_above_90() {
        // Values above 90° should be clamped.
        let [r1, g1, b1] = slope_to_terrain_rgb(90.0);
        let [r2, g2, b2] = slope_to_terrain_rgb(95.0);
        assert_eq!([r1, g1, b1], [r2, g2, b2]);
    }

    #[test]
    fn encode_webp_produces_bytes() {
        let slopes = vec![30.0f32; 4 * 4];
        let bytes = encode_slope_webp(&slopes, 4).expect("encode failed");
        assert!(!bytes.is_empty());
        // WebP files start with "RIFF" header.
        assert_eq!(&bytes[0..4], b"RIFF", "expected WebP RIFF header");
    }
}
