FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends nodejs npm pkg-config libssl-dev \
    && npm install -g pnpm@10 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY code ./code
COPY libs ./libs
COPY server ./server

RUN cargo build --release -p server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates liblzma5 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /workspace/target/release/server /app/server
COPY --from=builder /workspace/server/frontend/dist /app/static

EXPOSE 3000

ENTRYPOINT ["/app/server", "--static-dir", "/app/static"]
CMD ["-c", "/app/config.toml"]
