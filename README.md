# slope-server

A simple server that turns Terrain DEM tiles into slope angle tiles.

## Usage

```bash
> RUST_LOG=slope_server=debug \
  UPSTREAM_TILEJSON="https://tiles.mapterhorn.com/tilejson.json" \
  BIND_ADDR="0.0.0.0:8080" \
  OUTPUT_TILE_URL_BASE="https://example.com" \
  CACHE_MAX_TILES=1024 \
  CACHE_TTL_SECS=3600 \
  cargo run
```
