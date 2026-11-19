# Multi-stage Dockerfile for Garay WebRTC Signaling Server
# Build stage
FROM rust:1.83-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy Cargo files first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy source to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo fetch

# Copy actual source and build
COPY src ./src
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r garay && useradd -r -g garay -d /app -s /sbin/nologin garay

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/garay /app/garay

# Copy config example as default config
COPY config.toml.example /app/config.toml

# Create directory for TLS certificates (if mounted)
RUN mkdir -p /app/certs && chown -R garay:garay /app/certs

# Switch to non-root user
USER garay

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://127.0.0.1:3000/health || exit 1

# Run the server
ENTRYPOINT ["/app/garay"]