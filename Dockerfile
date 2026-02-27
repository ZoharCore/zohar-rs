# syntax=docker/dockerfile:1

FROM rust:bookworm AS chef

WORKDIR /work

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        protobuf-compiler libprotobuf-dev sqlite3 pkg-config libssl-dev ca-certificates \
        clang mold \
    && rm -rf /var/lib/apt/lists/*

# OPTIMIZATION: tell Cargo to use clang and the mold linker
ENV RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold"
ENV PROTOC=/usr/bin/protoc
ENV PROTOC_INCLUDE=/usr/include

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install --locked cargo-chef

FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cacher

COPY --from=planner /work/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --recipe-path recipe.json

FROM cacher AS builder

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release \
    -p zohar-content --bin content_runtime_cli \
    -p zohar-authsrv --bin zohar-auth \
    -p zohar-gamesrv --bin zohar-chgw \
    -p zohar-core --bin zohar-core \
    && mkdir -p /out/bin \
    && cp /work/target/release/content_runtime_cli /out/bin/content_runtime_cli \
    && cp /work/target/release/zohar-auth /out/bin/zohar-auth \
    && cp /work/target/release/zohar-chgw /out/bin/zohar-chgw \
    && cp /work/target/release/zohar-core /out/bin/zohar-core \
    && strip /out/bin/* # OPTIMIZATION: strip unused symbols to shrink image size

COPY .local/content ./data/content

RUN /out/bin/content_runtime_cli /out/content.db \
    && sqlite3 /out/content.db "PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;" \
    && rm -f /out/content.db-wal /out/content.db-shm

FROM debian:bookworm-slim AS runtime-base

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

FROM runtime-base AS auth-runtime

COPY --from=builder /out/bin/zohar-auth /usr/local/bin/zohar-auth

FROM runtime-base AS channel-runtime

COPY --from=builder /out/bin/zohar-chgw /usr/local/bin/zohar-chgw

FROM runtime-base AS core-runtime

COPY --from=builder /out/bin/zohar-core /usr/local/bin/zohar-core
RUN mkdir -p /var/lib/zohar
COPY --from=builder /out/content.db /var/lib/zohar/content.db

