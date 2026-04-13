#!/usr/bin/env bash
# Regenerate synthetic DEM test fixtures and gdaldem reference slope outputs.
#
# Requirements: python3, numpy, gdal (python bindings + CLI tools)
#   pip install numpy "gdal==3.12.0.post1"   # match system GDAL version
#
# Output files:
#   flat.bin           — 18×18 flat DEM (elevation = 100.0), raw f32 LE
#   ramp_ew.bin        — 18×18 DEM rising uniformly west→east, raw f32 LE
#   flat_slopes.bin    — gdaldem slope output for flat, 16×16 center, raw f32 LE
#   ramp_ew_slopes.bin — gdaldem slope output for ramp_ew, 16×16 center, raw f32 LE
#
# The DEM is 18×18 so it can be used directly as the padded (16+2)×(16+2)
# stitch buffer. gdaldem operates on the full 18×18 and we extract the center
# 16×16 region to match what our Rust pipeline produces.
#
# Scale: ewres = nsres = 1.0m, xscale = yscale = 1.0 (unit scale). This
# isolates the Horn kernel from the Mercator correction, which is tested
# separately in src/slope.rs unit tests.

set -euo pipefail
cd "$(dirname "$0")"

python3 - <<'PYTHON'
import struct, math, os, subprocess, tempfile
import numpy as np
from osgeo import gdal, osr

gdal.UseExceptions()

TILE = 16        # center tile size
PAD  = TILE + 2  # padded DEM size (18×18)
EWRES = 1.0      # pixel size in metres (unit scale)

def write_f32(path, data):
    flat = list(data) if not isinstance(data, list) else data
    with open(path, "wb") as f:
        f.write(struct.pack(f"<{len(flat)}f", *flat))

def make_geotiff(path, arr):
    """Write a float32 numpy array as a GeoTIFF with 1m pixel size."""
    rows, cols = arr.shape
    driver = gdal.GetDriverByName("GTiff")
    ds = driver.Create(path, cols, rows, 1, gdal.GDT_Float32)
    # Geotransform: origin (0,0), pixel (1m, -1m)
    ds.SetGeoTransform([0.0, EWRES, 0.0, float(rows), 0.0, -EWRES])
    # Projected CRS with metres — UTM zone 32N is fine for unit-scale testing.
    srs = osr.SpatialReference()
    srs.ImportFromEPSG(32632)
    ds.SetProjection(srs.ExportToWkt())
    ds.GetRasterBand(1).WriteArray(arr)
    ds.FlushCache()
    ds = None

def run_gdaldem_slope(src_tif, dst_tif):
    """Run gdaldem slope with Horn algorithm, scale=1."""
    subprocess.run(
        ["gdaldem", "slope", src_tif, dst_tif, "-alg", "Horn", "-s", "1",
         "-of", "GTiff", "-co", "COMPRESS=NONE"],
        check=True, capture_output=True,
    )

def read_center(tif_path):
    """Read the center TILE×TILE region from an 18×18 slope GeoTIFF."""
    ds = gdal.Open(tif_path)
    arr = ds.GetRasterBand(1).ReadAsArray()  # shape (18, 18)
    ds = None
    # gdaldem marks edge pixels as nodata; the center TILE×TILE region is valid.
    center = arr[1:TILE+1, 1:TILE+1]
    return center.astype(np.float32)

with tempfile.TemporaryDirectory() as tmp:
    # ── Flat DEM ─────────────────────────────────────────────────────────────
    flat_arr = np.full((PAD, PAD), 100.0, dtype=np.float32)
    write_f32("flat.bin", flat_arr.flatten())

    flat_tif   = os.path.join(tmp, "flat.tif")
    flat_s_tif = os.path.join(tmp, "flat_slope.tif")
    make_geotiff(flat_tif, flat_arr)
    run_gdaldem_slope(flat_tif, flat_s_tif)

    flat_slopes = read_center(flat_s_tif)
    write_f32("flat_slopes.bin", flat_slopes.flatten())
    print(f"flat: min={flat_slopes.min():.4f}° max={flat_slopes.max():.4f}°  (expected all 0)")

    # ── E-W ramp DEM ─────────────────────────────────────────────────────────
    # Elevation = column index (rises 1m per pixel west→east).
    cols_idx = np.tile(np.arange(PAD, dtype=np.float32), (PAD, 1))
    ramp_arr = cols_idx
    write_f32("ramp_ew.bin", ramp_arr.flatten())

    ramp_tif   = os.path.join(tmp, "ramp_ew.tif")
    ramp_s_tif = os.path.join(tmp, "ramp_ew_slope.tif")
    make_geotiff(ramp_tif, ramp_arr)
    run_gdaldem_slope(ramp_tif, ramp_s_tif)

    ramp_slopes = read_center(ramp_s_tif)
    write_f32("ramp_ew_slopes.bin", ramp_slopes.flatten())
    print(f"ramp_ew: min={ramp_slopes.min():.4f}° max={ramp_slopes.max():.4f}°  (expected all ~45)")
    assert abs(ramp_slopes.mean() - 45.0) < 0.01, f"unexpected mean slope: {ramp_slopes.mean()}"

print("All fixtures written and sanity-checked.")
PYTHON
