FROM --platform=$BUILDPLATFORM debian:trixie-slim AS builder-tools

ARG RUST_NIGHTLY_VERSION=nightly-2025-12-01
ARG ZIG_VERSION=0.16.0-dev.1859+212968c57

ENV CARGO_HOME=/cargo \
    RUSTUP_HOME=/rustup \
    PATH="/cargo/bin:/rustup/toolchains/${RUST_NIGHTLY_VERSION}/bin:/zig:$PATH"

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates curl xz-utils build-essential \
    && rm -rf /var/lib/apt/lists/* \
    && curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain ${RUST_NIGHTLY_VERSION} \
    && curl -L https://ziglang.org/builds/zig-x86_64-linux-${ZIG_VERSION}.tar.xz | tar -xJ && mv zig-x86_64-linux-${ZIG_VERSION} /zig \
    && cargo install --locked cargo-zigbuild --version 0.20.1 \
    && cargo install --locked cargo-chef --version 0.1.73 \
    && rustup target add \
        x86_64-unknown-linux-musl \
        aarch64-unknown-linux-musl \
        x86_64-unknown-linux-gnu \
        aarch64-unknown-linux-gnu

## Actual builder base ##
FROM builder-tools AS builder-base

ENV SERVER_SHARED_PREBUILT_DATA=1
WORKDIR /app

# prepare the build cache
COPY src ./src
COPY Cargo.toml Cargo.lock ./
RUN cargo chef prepare --recipe-path recipe.json

## Musl ##
FROM builder-base AS builder-musl
ARG TARGETARCH

# map arch to target
RUN case "$TARGETARCH" in \
    amd64) echo "x86_64-unknown-linux-musl" > /target.txt ;; \
    arm64) echo "aarch64-unknown-linux-musl" > /target.txt ;; \
    *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac

# build dependencies
RUN cargo chef cook --release --zigbuild --target $(cat /target.txt) --features mimalloc --recipe-path recipe.json

# build the project
COPY src ./src
COPY Cargo.toml Cargo.lock ./
RUN cargo zigbuild --release --features mimalloc --target $(cat /target.txt)

## glibc ##
FROM builder-base AS builder-glibc
ARG TARGETARCH

# map arch to target
RUN case "$TARGETARCH" in \
    amd64) echo "x86_64-unknown-linux-gnu" > /target.txt ;; \
    arm64) echo "aarch64-unknown-linux-gnu" > /target.txt ;; \
    *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac

# build dependencies
RUN cargo chef cook --release --zigbuild --target $(cat /target.txt) --features mimalloc --recipe-path recipe.json

# build the project
COPY src ./src
COPY Cargo.toml Cargo.lock ./
RUN cargo zigbuild --release --features mimalloc --target $(cat /target.txt)

## alpine runtime ##
FROM alpine:latest AS runtime-alpine
COPY --from=builder-musl /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENV INSIDE_DOCKER=1
ENTRYPOINT ["/game-server"]

## debian runtime ##
FROM debian:trixie-slim AS runtime-debian
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder-glibc /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENV INSIDE_DOCKER=1
ENTRYPOINT ["/game-server"]
