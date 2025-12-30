FROM --platform=$BUILDPLATFORM rustlang/rust:nightly AS builder-base

ENV SERVER_SHARED_PREBUILT_DATA=1

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates curl xz-utils
RUN rm -rf /var/lib/apt/lists/*

# download zig
RUN curl -L https://ziglang.org/builds/zig-x86_64-linux-0.16.0-dev.1859+212968c57.tar.xz | tar -xJ && mv zig-x86_64-linux-0.16.0-dev.1859+212968c57 /zig
ENV PATH="/zig:${PATH}"

# install zigbuild
RUN cargo install --locked cargo-zigbuild

# copy the server
COPY src ./src
COPY Cargo.toml Cargo.lock ./

## Musl ##
FROM builder-base AS builder-musl
ARG TARGETARCH

# map arch to target
RUN case "$TARGETARCH" in \
    amd64) echo "x86_64-unknown-linux-musl" > /target.txt ;; \
    arm64) echo "aarch64-unknown-linux-musl" > /target.txt ;; \
    *) echo "unsupported architecture" >&2; exit 1 ;; \
    esac

# install target and build
RUN rustup target add $(cat /target.txt)
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

# install target and build
RUN rustup target add $(cat /target.txt)
RUN cargo zigbuild --release --features mimalloc --target $(cat /target.txt)

## alpine runtime ##
FROM alpine:latest AS runtime-alpine
COPY --from=builder-musl /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENTRYPOINT ["/game-server"]

## debian runtime ##
FROM debian:stable-slim AS runtime-debian
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder-glibc /app/target/*/release/game-server /game-server

EXPOSE 4349/tcp
EXPOSE 4349/udp

ENTRYPOINT ["/game-server"]
