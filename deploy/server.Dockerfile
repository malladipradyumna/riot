# Build on the deployment architecture; the bundled protoc supports ARM64 too.
FROM rust:1.95-bookworm AS build
WORKDIR /build
COPY server/Cargo.toml server/Cargo.lock server/build.rs ./server/
COPY server/src ./server/src
COPY assets/protocol ./assets/protocol
RUN cargo build --locked --release --manifest-path server/Cargo.toml --bin server

# The runtime image contains the binary and shared assets, not the Rust toolchain.
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /build/server/target/release/server /app/server
COPY assets /app/assets
COPY LICENSE THIRD_PARTY_NOTICES.md /app/
ENV RIOT_ADDRESS=0.0.0.0:8000 RIOT_ASSETS_DIR=/app/assets RIOT_BOT_PATHS_DIR=/tmp/bot-paths
# Run without root; Compose mounts /tmp for the generated route previews.
USER 10001:10001
EXPOSE 8000
CMD ["/app/server"]
