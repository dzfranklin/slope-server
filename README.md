# slope-server

A simple server that turns Terrain DEM tiles into slope angle tiles.

## Usage

```bash
docker run -p 8080:8080 \
  -e UPSTREAM_TILEJSON="https://tiles.mapterhorn.com/tilejson.json" \
  -e OUTPUT_TILE_URL_BASE="https://example.com" \
  ghcr.io/dzfranklin/slope-server:latest
```

**Running locally**

```bash
RUST_LOG=slope_server=debug \
  UPSTREAM_TILEJSON="https://tiles.mapterhorn.com/tilejson.json" \
  BIND_ADDR="0.0.0.0:8080" \
  OUTPUT_TILE_URL_BASE="https://example.com" \
  CACHE_MAX_TILES=1024 \
  CACHE_TTL_SECS=3600 \
  cargo run
```

```bash
docker build -t slope-server .
docker run -p 8080:8080 slope-server
```

Visit http://localhost:8080/demo

## Development

```bash
git config core.hooksPath .githooks
```
