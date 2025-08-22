#!/bin/bash
set -euo pipefail

RUST_IMAGE="rust:bullseye"
HOST_DIR="$(pwd)"
WORKDIR="/work"

docker run --rm \
    -v "$HOST_DIR:$WORKDIR" \
    -v "$HOST_DIR/docker-target:$WORKDIR/target" \
    -w "$WORKDIR" \
    "$RUST_IMAGE" \
    bash "docker-build-inner.sh"

echo "Docker build done!"
