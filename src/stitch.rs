use std::sync::Arc;

/// Decoded elevation grid for a single source tile.
pub type ElevationTile = Arc<Vec<f32>>;

/// Assemble a padded (tile_size+2)×(tile_size+2) elevation buffer from a 3×3
/// grid of optional source tiles.
///
/// `tiles[row][col]` — row 0 = north neighbor, row 2 = south neighbor;
/// col 0 = west, col 2 = east. `tiles[1][1]` is the center tile and must be
/// `Some`.
///
/// Missing neighbors (None) are filled by replicating the nearest edge pixels
/// of the center tile, so the Horn kernel always has valid data at every
/// border pixel. Missing corners replicate the nearest center tile corner.
///
/// The returned buffer is row-major with stride `tile_size + 2`. The center
/// tile occupies rows [1..=tile_size], cols [1..=tile_size].
pub fn stitch_padded(tiles: [[Option<ElevationTile>; 3]; 3], tile_size: u32) -> Vec<f32> {
    let n = tile_size as usize;
    let padded = n + 2;
    let mut buf = vec![0.0f32; padded * padded];

    // Helper: get elevation from a tile at grid position (tr, tc) at local
    // pixel (r, c) — clamped to [0, n-1]. When the tile is absent, falls back
    // to the nearest edge pixel of the center tile: the edge row/col closest
    // to the missing neighbor, at the same r or c as requested (clamped).
    let get = |tiles: &[[Option<ElevationTile>; 3]; 3],
               tr: usize,
               tc: usize,
               r: usize,
               c: usize|
     -> f32 {
        let r = r.min(n - 1);
        let c = c.min(n - 1);
        match &tiles[tr][tc] {
            Some(t) => t[r * n + c],
            None => {
                // Pick the nearest center-tile edge row and col.
                // For a north neighbor (tr=0) missing: use center's top row (cr=0), same col.
                // For a south neighbor (tr=2) missing: use center's bottom row (cr=n-1), same col.
                // For a west  neighbor (tc=0) missing: use center's left col (cc=0), same row.
                // For an east neighbor (tc=2) missing: use center's right col (cc=n-1), same row.
                // Corners: both row and col snap to the nearest corner of center.
                let cr = match tr {
                    0 => 0,
                    2 => n - 1,
                    _ => r,
                };
                let cc = match tc {
                    0 => 0,
                    2 => n - 1,
                    _ => c,
                };
                match &tiles[1][1] {
                    Some(t) => t[cr * n + cc],
                    None => 0.0,
                }
            }
        }
    };

    // ── Fill center tile ──────────────────────────────────────────────────────
    if let Some(center) = &tiles[1][1] {
        for row in 0..n {
            for col in 0..n {
                buf[(row + 1) * padded + (col + 1)] = center[row * n + col];
            }
        }
    }

    // ── Top border row (padded row 0) ─────────────────────────────────────────
    // Left corner: from NW tile bottom-right pixel (or fallback)
    buf[0] = get(&tiles, 0, 0, n - 1, n - 1);
    // Top edge: from N tile bottom row
    for col in 0..n {
        buf[col + 1] = get(&tiles, 0, 1, n - 1, col);
    }
    // Right corner: from NE tile bottom-left pixel (or fallback)
    buf[n + 1] = get(&tiles, 0, 2, n - 1, 0);

    // ── Bottom border row (padded row n+1) ────────────────────────────────────
    let base = (n + 1) * padded;
    buf[base] = get(&tiles, 2, 0, 0, n - 1);
    for col in 0..n {
        buf[base + col + 1] = get(&tiles, 2, 1, 0, col);
    }
    buf[base + n + 1] = get(&tiles, 2, 2, 0, 0);

    // ── Left and right border columns ─────────────────────────────────────────
    for row in 0..n {
        // Left border: from W tile right column
        buf[(row + 1) * padded] = get(&tiles, 1, 0, row, n - 1);
        // Right border: from E tile left column
        buf[(row + 1) * padded + n + 1] = get(&tiles, 1, 2, row, 0);
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tile(val: f32, n: usize) -> ElevationTile {
        Arc::new(vec![val; n * n])
    }

    fn make_tile_fn(n: usize, f: impl Fn(usize, usize) -> f32) -> ElevationTile {
        let mut v = Vec::with_capacity(n * n);
        for r in 0..n {
            for c in 0..n {
                v.push(f(r, c));
            }
        }
        Arc::new(v)
    }

    #[test]
    fn all_same_value_fills_correctly() {
        let n = 4usize;
        let padded = n + 2;
        // All 9 tiles with value 42.0
        let tiles: [[Option<ElevationTile>; 3]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| Some(make_tile(42.0, n))));
        let buf = stitch_padded(tiles, n as u32);
        assert_eq!(buf.len(), padded * padded);
        // Every cell should be 42.0
        for (i, &v) in buf.iter().enumerate() {
            assert_eq!(v, 42.0, "buf[{i}] = {v}");
        }
    }

    #[test]
    fn missing_neighbor_uses_center_edge() {
        let n = 4usize;
        let padded = n + 2;
        // Center tile: row * 10 + col
        let center = make_tile_fn(n, |r, c| (r * 10 + c) as f32);
        let mut tiles: [[Option<ElevationTile>; 3]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| None));
        tiles[1][1] = Some(center);
        let buf = stitch_padded(tiles, n as u32);

        // Top border (padded row 0) should replicate center tile's top row (row 0).
        // buf[0 * padded + 1] = center[0 * n + 0] = 0
        // buf[0 * padded + 2] = center[0 * n + 1] = 1
        for col in 0..n {
            let expected = (0 * 10 + col) as f32; // center top row
            let got = buf[0 * padded + col + 1];
            assert_eq!(
                got, expected,
                "top border col {col}: got {got}, expected {expected}"
            );
        }

        // Left border (padded col 0) should replicate center tile's left col (col 0).
        for row in 0..n {
            let expected = (row * 10 + 0) as f32;
            let got = buf[(row + 1) * padded + 0];
            assert_eq!(got, expected, "left border row {row}: got {got}");
        }
    }

    #[test]
    fn center_values_land_in_correct_positions() {
        let n = 2usize;
        let padded = n + 2; // 4
        let center = make_tile_fn(n, |r, c| (r * 10 + c) as f32);
        let mut tiles: [[Option<ElevationTile>; 3]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| None));
        tiles[1][1] = Some(center);
        let buf = stitch_padded(tiles, n as u32);

        // Center tile is at rows [1..=n], cols [1..=n] in padded buffer.
        assert_eq!(buf[1 * padded + 1], 0.0); // center[0][0]
        assert_eq!(buf[1 * padded + 2], 1.0); // center[0][1]
        assert_eq!(buf[2 * padded + 1], 10.0); // center[1][0]
        assert_eq!(buf[2 * padded + 2], 11.0); // center[1][1]
    }
}
