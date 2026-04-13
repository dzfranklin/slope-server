#!/usr/bin/env python3
"""
Generate gdaldem slope reference for the Buachaille Etive Mòr tile
(z=12, x=1992, y=1261) using Terrarium decoding.

Writes:
  buachaille_padded.bin   — (512+2)×(512+2) f32 LE, the stitched padded DEM
  buachaille_slopes.bin   — 512×512 f32 LE, gdaldem Horn slope in degrees

Run from testdata/:
  python3 gen_buachaille_ref.py
"""

import math, struct, subprocess, tempfile, os
import numpy as np
from PIL import Image
from osgeo import gdal, osr

gdal.UseExceptions()

Z, CX, CY = 12, 1992, 1261
TILE = 512
PAD  = TILE + 2

# ── Terrarium decode ──────────────────────────────────────────────────────────
def decode_terrarium(path):
    img = Image.open(path).convert("RGB")
    arr = np.array(img, dtype=np.float32)
    r, g, b = arr[:,:,0], arr[:,:,1], arr[:,:,2]
    return r * 256.0 + g - 32768.0 + b / 256.0

# ── Load 3×3 grid ─────────────────────────────────────────────────────────────
grid = {}
for dy in range(-1, 2):
    for dx in range(-1, 2):
        x, y = CX + dx, CY + dy
        path = f"buachaille_12_{x}_{y}.webp"
        grid[(dy, dx)] = decode_terrarium(path)
        print(f"loaded 12/{x}/{y}: elev range [{grid[(dy,dx)].min():.1f}, {grid[(dy,dx)].max():.1f}]m")

# ── Stitch padded buffer ──────────────────────────────────────────────────────
# (TILE+2)×(TILE+2), row-major. Center tile at rows [1..TILE], cols [1..TILE].
padded = np.zeros((PAD, PAD), dtype=np.float32)

# Center tile
padded[1:TILE+1, 1:TILE+1] = grid[(0, 0)]

# Edge neighbors
padded[0,      1:TILE+1] = grid[(-1, 0)][-1, :]   # north → bottom row
padded[TILE+1, 1:TILE+1] = grid[( 1, 0)][ 0, :]   # south → top row
padded[1:TILE+1, 0]      = grid[( 0,-1)][:, -1]   # west  → right col
padded[1:TILE+1, TILE+1] = grid[( 0, 1)][:,  0]   # east  → left col

# Corner neighbors
padded[0,      0]      = grid[(-1,-1)][-1, -1]
padded[0,      TILE+1] = grid[(-1, 1)][-1,  0]
padded[TILE+1, 0]      = grid[( 1,-1)][ 0, -1]
padded[TILE+1, TILE+1] = grid[( 1, 1)][ 0,  0]

# Write padded DEM
padded.flatten().astype('<f4').tofile("buachaille_padded.bin")
print(f"\npadded buffer: {PAD}×{PAD}, elev range [{padded.min():.1f}, {padded.max():.1f}]m")

# ── Mercator georeferencing for z=12, y=1261 ──────────────────────────────────
# We need gdaldem to use the correct ground pixel size so slope is accurate.
# ewres = nsres = 2π*R / (TILE * 2^Z)
R = 6_378_137.0
ewres = 2 * math.pi * R / (TILE * 2**Z)
print(f"ewres = {ewres:.4f} m/px")

# Mercator scale correction: xscale = yscale = cos(lat_center)
# lat_center of tile y=1261 at z=12
n = 2**Z
lat_center = math.atan(math.sinh(math.pi * (1 - 2 * (CY + 0.5) / n)))
cos_lat = math.cos(lat_center)
print(f"lat_center = {math.degrees(lat_center):.4f}°, cos_lat = {cos_lat:.6f}")

# True ground pixel size at this latitude
ground_res = ewres * cos_lat
print(f"ground resolution = {ground_res:.4f} m/px")

# The padded array origin in Mercator metres.
# Tile (CX, CY) at zoom Z: west edge x_m, north edge y_m in EPSG:3857.
circumference = 2 * math.pi * R
tile_size_m = circumference / 2**Z  # metres per tile
x_origin_m = (CX - 1) * tile_size_m - circumference / 2   # one tile west (for padding col)
y_origin_m = circumference / 2 - (CY - 1) * tile_size_m   # one tile north (for padding row)

# ── Write padded buffer as GeoTIFF ────────────────────────────────────────────
with tempfile.TemporaryDirectory() as tmp:
    dem_tif   = os.path.join(tmp, "padded.tif")
    slope_tif = os.path.join(tmp, "slope.tif")

    driver = gdal.GetDriverByName("GTiff")
    ds = driver.Create(dem_tif, PAD, PAD, 1, gdal.GDT_Float32)

    # Geotransform: use the nominal Mercator pixel size (ewres), not the
    # corrected ground resolution. The Mercator correction is applied via -s
    # below, matching how our Rust mercator_scale_factors works.
    ds.SetGeoTransform([x_origin_m, ewres, 0.0, y_origin_m, 0.0, -ewres])

    srs = osr.SpatialReference()
    srs.ImportFromEPSG(3857)
    ds.SetProjection(srs.ExportToWkt())
    ds.GetRasterBand(1).WriteArray(padded)
    ds.FlushCache()
    ds = None

    # -s cos_lat applies the Mercator correction: gdaldem divides ewres by
    # cos_lat to get the true ground distance, matching our Rust
    # mercator_scale_factors which computes 1 / (ewres * cos_lat).
    subprocess.run(
        ["gdaldem", "slope", dem_tif, slope_tif,
         "-alg", "Horn", "-s", str(cos_lat), "-of", "GTiff", "-co", "COMPRESS=NONE"],
        check=True, capture_output=True,
    )

    # Read center TILE×TILE region (gdaldem marks 1px border as nodata).
    ds2 = gdal.Open(slope_tif)
    slope_arr = ds2.GetRasterBand(1).ReadAsArray().astype(np.float32)
    ds2 = None
    center_slopes = slope_arr[1:TILE+1, 1:TILE+1]

center_slopes.flatten().astype('<f4').tofile("buachaille_slopes.bin")
print(f"slopes: min={center_slopes.min():.2f}° max={center_slopes.max():.2f}° mean={center_slopes.mean():.2f}°")
print("\nDone. Committed artifacts:")
print("  buachaille_padded.bin")
print("  buachaille_slopes.bin")
