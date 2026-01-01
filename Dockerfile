FROM --platform=$BUILDPLATFORM debian:trixie-slim AS builder-base

ARG RUST_NIGHTLY_VERSION=nightly-2025-12-01
ARG ZIG_VERSION=0.16.0-dev.1859+212968c57

ENV SERVER_SHARED_PREBUILT_DATA=1 \
    CARGO_HOME=/cargo \
    RUSTUP_HOME=/rustup \
    PATH="/cargo/bin:/rustup/toolchains/${RUST_NIGHTLY_VERSION}/bin:$PATH"

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates curl xz-utils build-essential \
    && rm -rf /var/lib/apt/lists/*
RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
RUN rustup toolchain install ${RUST_NIGHTLY_VERSION} && rustup default ${RUST_NIGHTLY_VERSION}

# download zig
RUN curl -L https://ziglang.org/builds/zig-x86_64-linux-${ZIG_VERSION}.tar.xz | tar -xJ && mv zig-x86_64-linux-${ZIG_VERSION} /zig
ENV PATH="/zig:${PATH}"

# install zigbuild and cargo chef
RUN cargo install --locked cargo-zigbuild cargo-chef

# prepare the build cache
COPY . .
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
RUN rustup target add $(cat /target.txt) && \
    rm -rf src && \
    cargo chef cook --release --zigbuild --target $(cat /target.txt) --features mimalloc --recipe-path recipe.json

# build the project
COPY . .
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
RUN rustup target add $(cat /target.txt) && \
    rm -rf src && \
    cargo chef cook --release --zigbuild --target $(cat /target.txt) --features mimalloc --recipe-path recipe.json

# build the project
COPY src ./src
RUN cargo zigbuild --release --features mimalloc --target $(cat /target.txt)

## alpine runtime ##
FROM alpine:latest AS runtime-alpine
COPY --from=builder-musl /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENTRYPOINT ["/game-server"]

## debian runtime ##
FROM debian:trixie-slim AS runtime-debian
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder-glibc /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENTRYPOINT ["/game-server"]
