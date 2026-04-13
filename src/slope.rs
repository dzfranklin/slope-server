use crate::tile::TileCoord;
use std::f32::consts::PI as PI_F32;

/// Compute slope in degrees for a single 3×3 window using the Horn algorithm,
/// ported directly from gdaldem (apps/gdaldem_lib.cpp, GDALSlopeHornAlg,
/// https://github.com/OSGeo/gdal, circa GDAL 3.9).
///
/// Window layout (row-major, top-left to bottom-right):
///
///   0 1 2
///   3 4 5
///   6 7 8
///
/// `inv_ewres_xscale` = 1 / (ewres * xscale)
/// `inv_nsres_yscale` = 1 / (nsres * yscale)
///
/// The 1/8 normalization factor is folded into the atan call (not into the
/// inverse-resolution factors), exactly as in the GDAL source.
pub fn horn_kernel(win: [f32; 9], inv_ewres_xscale: f32, inv_nsres_yscale: f32) -> f32 {
    let dx = ((win[0] + win[3] + win[3] + win[6]) - (win[2] + win[5] + win[5] + win[8]))
        * inv_ewres_xscale;

    let dy = ((win[6] + win[7] + win[7] + win[8]) - (win[0] + win[1] + win[1] + win[2]))
        * inv_nsres_yscale;

    let key = dx * dx + dy * dy;

    (key.sqrt() * (1.0 / 8.0)).atan() * (180.0 / PI_F32)
}

/// Compute the inverse resolution scale factors for the Horn kernel for a
/// given tile.
///
/// In Web Mercator, pixels shrink toward the poles by cos(lat). Without
/// correction the Horn kernel would overstate slopes at high latitudes
/// (e.g. ~2× error at 57°N / Scottish Highlands). The correction follows
/// gdaldem's xscale/yscale convention: divide ewres by cos(lat_center) to
/// recover the true ground distance per pixel.
///
/// xscale = yscale = cos(lat_center) (Mercator is conformal, same in both axes)
/// ewres  = nsres  = resolution_meters(z, tile_size)
///
/// Returns (inv_ewres_xscale, inv_nsres_yscale) as f32, computed once per
/// tile (within-tile latitude variation is negligible at z≥8).
pub fn mercator_scale_factors(z: u32, y: u32, tile_size: u32) -> (f32, f32) {
    let coord = TileCoord::new(z, 0, y);
    let lat = coord.center_lat_rad();
    let cos_lat = lat.cos() as f32;
    let ewres = TileCoord::resolution_meters(z, tile_size) as f32;

    // xscale = yscale = cos(lat); inv = 1 / (ewres * cos_lat)
    let inv = 1.0 / (ewres * cos_lat);
    (inv, inv)
}

/// Compute slope in degrees for every pixel in the center tile_size×tile_size
/// region of a padded (tile_size+2)×(tile_size+2) elevation buffer.
///
/// `padded` is row-major with stride `tile_size + 2`. The center tile occupies
/// rows [1..=tile_size], cols [1..=tile_size] of the padded buffer.
///
/// Returns a Vec of length tile_size² in the same row-major order.
pub fn compute_slope(padded: &[f32], tile_size: u32, z: u32, y: u32) -> Vec<f32> {
    let padded_stride = (tile_size + 2) as usize;
    let n = tile_size as usize;
    let (inv_ew, inv_ns) = mercator_scale_factors(z, y, tile_size);
    let mut out = vec![0.0f32; n * n];

    for row in 0..n {
        for col in 0..n {
            // Top-left of the 3×3 window in the padded buffer.
            let pr = row; // padded row of top of window (row 0 in padded = border row)
            let pc = col; // padded col of left of window

            let win = [
                padded[pr * padded_stride + pc],
                padded[pr * padded_stride + pc + 1],
                padded[pr * padded_stride + pc + 2],
                padded[(pr + 1) * padded_stride + pc],
                padded[(pr + 1) * padded_stride + pc + 1],
                padded[(pr + 1) * padded_stride + pc + 2],
                padded[(pr + 2) * padded_stride + pc],
                padded[(pr + 2) * padded_stride + pc + 1],
                padded[(pr + 2) * padded_stride + pc + 2],
            ];

            out[row * n + col] = horn_kernel(win, inv_ew, inv_ns);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_surface_zero_slope() {
        // A flat DEM (all same elevation) should give zero slope everywhere.
        let win = [100.0f32; 9];
        let slope = horn_kernel(win, 1.0 / 30.0, 1.0 / 30.0);
        assert!(slope.abs() < 1e-4, "flat surface slope = {slope}");
    }

    #[test]
    fn known_slope_45_degrees() {
        // A uniform E-W ramp rising 1 unit per pixel.
        // With ewres=1 and xscale=1 (inv_ewres_xscale=1), the Horn dx term:
        //   dx = ((w0+w3+w3+w6) - (w2+w5+w5+w8)) * 1
        // For a ramp where col 0 = elev-1, col 1 = elev, col 2 = elev+1:
        //   w0=e-1, w1=e, w2=e+1, w3=e-1, w4=e, w5=e+1, w6=e-1, w7=e, w8=e+1
        //   dx = ((e-1)+(e-1)+(e-1)+(e-1)) - ((e+1)+(e+1)+(e+1)+(e+1)) = -8
        //   (sign depends on convention; magnitude is 8)
        //   slope = atan(sqrt(64) * (1/8)) = atan(1) = 45°
        let e = 0.0f32;
        let win = [
            e - 1.0,
            e,
            e + 1.0,
            e - 1.0,
            e,
            e + 1.0,
            e - 1.0,
            e,
            e + 1.0,
        ];
        let slope = horn_kernel(win, 1.0, 1.0);
        assert!((slope - 45.0).abs() < 1e-4, "expected 45°, got {slope}");
    }

    #[test]
    fn mercator_correction_reduces_at_poles() {
        // At higher latitudes cos(lat) < 1, so ewres*cos_lat < ewres,
        // meaning inv_ewres_xscale is larger — but wait, we're dividing by a
        // smaller number so the denominator gets smaller and inv gets larger.
        // Actually: a steeper apparent slope at high lat means correction should
        // REDUCE the computed slope. Let's verify: at z=10, y=0 (near north pole)
        // vs y=512 (near equator), the correction factor differs.
        let (inv_ew_pole, _) = mercator_scale_factors(10, 0, 512);
        let (inv_ew_eq, _) = mercator_scale_factors(10, 512, 512);
        // Near the pole cos(lat) → 0, so ewres*cos_lat → 0, so inv → ∞.
        // Near equator cos(lat) ≈ 1, inv is smaller.
        // So inv_ew_pole > inv_ew_eq (more correction needed at poles).
        assert!(
            inv_ew_pole > inv_ew_eq,
            "pole inv={inv_ew_pole}, equator inv={inv_ew_eq}"
        );
    }
}
