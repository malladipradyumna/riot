FROM rust:1.95-bookworm AS build
WORKDIR /build
COPY server/Cargo.toml server/Cargo.lock server/build.rs ./server/
COPY server/src ./server/src
COPY assets/protocol ./assets/protocol
RUN cargo build --locked --release --manifest-path server/Cargo.toml --bin server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /build/server/target/release/server /app/server
COPY assets /app/assets
COPY LICENSE THIRD_PARTY_NOTICES.md /app/
ENV RIOT_ADDRESS=0.0.0.0:8000 RIOT_METRICS_ADDRESS=0.0.0.0:9000 RIOT_ASSETS_DIR=/app/assets
USER 10001:10001
EXPOSE 8000 9000
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 \
    CMD curl --fail --silent http://127.0.0.1:9000/healthz || exit 1
CMD ["/app/server"]
