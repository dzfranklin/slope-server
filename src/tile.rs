use std::f64::consts::PI;

/// A Web Mercator tile coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileCoord {
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

impl TileCoord {
    pub fn new(z: u32, x: u32, y: u32) -> Self {
        Self { z, x, y }
    }

    /// Returns the 3×3 neighbor grid centered on self, in row-major order
    /// (top-left to bottom-right). X wraps at the antimeridian; Y is clamped
    /// (None for out-of-range rows).
    pub fn neighbors_3x3(&self) -> [[Option<TileCoord>; 3]; 3] {
        let max = (1u32 << self.z).saturating_sub(1);
        let mut grid = [[None; 3]; 3];
        for dy in 0i32..3 {
            let ny = self.y as i32 + dy - 1;
            if ny < 0 || ny > max as i32 {
                // Entire row is out of range — leave as None.
                continue;
            }
            for dx in 0i32..3 {
                // X wraps: use rem_euclid to handle negative values cleanly.
                let nx = (self.x as i32 + dx - 1).rem_euclid(max as i32 + 1) as u32;
                grid[dy as usize][dx as usize] = Some(TileCoord {
                    z: self.z,
                    x: nx,
                    y: ny as u32,
                });
            }
        }
        grid
    }

    /// Latitude of the tile center in radians (Web Mercator).
    pub fn center_lat_rad(&self) -> f64 {
        let n = 1u32 << self.z;
        // Fractional tile position of center: (y + 0.5) / 2^z
        let frac = (self.y as f64 + 0.5) / n as f64;
        // Inverse Mercator: lat = atan(sinh(π * (1 - 2*frac)))
        (PI * (1.0 - 2.0 * frac)).sinh().atan()
    }

    /// Ground resolution in meters per pixel for a given zoom and tile size.
    /// Formula: 2π * R_earth / (tile_size * 2^z)
    pub fn resolution_meters(z: u32, tile_size: u32) -> f64 {
        let earth_circumference = 2.0 * PI * 6_378_137.0;
        let n = (1u64 << z) as f64;
        earth_circumference / (tile_size as f64 * n)
    }

    /// Bounding box of this tile in WGS84 degrees: [west, south, east, north].
    pub fn tile_bounds(&self) -> [f64; 4] {
        let n = (1u32 << self.z) as f64;

        let west = self.x as f64 / n * 360.0 - 180.0;
        let east = (self.x + 1) as f64 / n * 360.0 - 180.0;

        let north_rad = (PI * (1.0 - 2.0 * self.y as f64 / n)).sinh().atan();
        let south_rad = (PI * (1.0 - 2.0 * (self.y + 1) as f64 / n)).sinh().atan();

        let north = north_rad.to_degrees();
        let south = south_rad.to_degrees();

        [west, south, east, north]
    }

    /// Returns true if this tile's bounding box intersects the given bounds
    /// [west, south, east, north] in WGS84 degrees.
    pub fn intersects_bounds(&self, bounds: &[f64; 4]) -> bool {
        let [west, south, east, north] = self.tile_bounds();
        let [bw, bs, be, bn] = *bounds;
        west < be && east > bw && south < bn && north > bs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn center_lat_equator() {
        // z=1, y=1 straddles the equator; center should be near 0 but negative
        // (southern half). z=0, y=0 covers the whole world, center at ~0.
        let t = TileCoord::new(0, 0, 0);
        // Center of the whole world tile is at lat = atan(sinh(0)) = 0
        assert!((t.center_lat_rad()).abs() < 1e-10);
    }

    #[test]
    fn resolution_z0() {
        // At z=0 with 512px tiles the whole earth circumference fits in one tile.
        let res = TileCoord::resolution_meters(0, 512);
        let expected = 2.0 * PI * 6_378_137.0 / 512.0;
        assert!((res - expected).abs() < 1e-6);
    }

    #[test]
    fn neighbors_3x3_wraps_x() {
        // At z=1 there are 2 tiles in X (0 and 1). Tile (1,0,0) is top-left.
        // y=0 means the row above (y=-1) is out of range → entire top row None.
        // X wraps: left neighbor of x=0 is x=1 (max).
        let t = TileCoord::new(1, 0, 0);
        let grid = t.neighbors_3x3();
        // Top row: y=-1 is out of range, all None.
        assert!(grid[0].iter().all(|c| c.is_none()));
        // Middle-left (x wraps to 1): grid[1][0]
        assert_eq!(grid[1][0], Some(TileCoord::new(1, 1, 0)));
        // Middle-center (self): grid[1][1]
        assert_eq!(grid[1][1], Some(TileCoord::new(1, 0, 0)));
        // Middle-right (x=1): grid[1][2]
        assert_eq!(grid[1][2], Some(TileCoord::new(1, 1, 0)));
        // Bottom-left (x wraps, y=1): grid[2][0]
        assert_eq!(grid[2][0], Some(TileCoord::new(1, 1, 1)));
    }

    #[test]
    fn neighbors_3x3_clamps_y() {
        // Top row of tiles: y=0. The row above (y=-1) should all be None.
        let t = TileCoord::new(2, 2, 0);
        let grid = t.neighbors_3x3();
        assert!(grid[0].iter().all(|c| c.is_none()));
        // Bottom row exists.
        assert!(grid[2].iter().all(|c| c.is_some()));
    }

    #[test]
    fn tile_bounds_sanity() {
        // z=0, x=0, y=0 should cover the whole world.
        let t = TileCoord::new(0, 0, 0);
        let [west, south, east, north] = t.tile_bounds();
        assert!((west - (-180.0)).abs() < 1e-6);
        assert!((east - 180.0).abs() < 1e-6);
        // Web Mercator clips to ~85.05° rather than 90°.
        assert!(north > 85.0);
        assert!(south < -85.0);
    }
}
