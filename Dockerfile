# Build stage
FROM rust:1.94-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/target/release/slope-server /usr/local/bin/slope-server

ENV RUST_LOG=slope_server=info
ENV BIND_ADDR=0.0.0.0:8080

# Expose the default port
EXPOSE 8080

ENTRYPOINT ["slope-server"]
