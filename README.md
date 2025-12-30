# Globed Game Server

## Building

Nightly rust is required. If you have never used Rust, install rustup from https://rustup.rs/ and then ensure you have the latest version by running
```sh
rustup toolchain install nightly
```

While in the server folder, add an override to tell cargo to always use nightly Rust for building the server:
```sh
rustup override set nightly
```

[Cap 'n Proto](https://capnproto.org/install.html) is recommended but not required. If you have troubles installing it, or are getting `error: Import failed: /capnp/c++.capnp` errors during the server build, you can set the `SERVER_SHARED_PREBUILT_DATA=1` environment variable to remove the need for capnp. **This is only recommended to do if you aren't planning on changing the schemas!**

To build in default configuration, with base features:
```sh
cargo build # add --release for release builds
```

To build with extra features, `--features` can be passed:
```sh
cargo build --features scripting,mimalloc
```

Possible feature flags:
* `all` - includes all functional features, does not include `stat-tracking` and `mimalloc`
* `scripting` - enables the Lua scripting module. **this feature currently requires closed-source code, and will NOT build!**
* `stat-tracking` - **for debugging only** tracks every packet of every connection. on Linux, completed connections can be dumped by sending `SIGUSR1`
* `mimalloc` - replaces the allocator with MiMalloc

## Docker builds

Docker buildx can be used to build an image of the server:
```sh
docker buildx build --target <target> --platform <platform> -t game-server:latest .
# to save the image
docker save -o image.tar game-server:latest
```

`<target>` must be either `runtime-alpine` (static linked musl binary, small alpine image) or `runtime-debian` (glibc linked binary, debian-slim runtime)

`<platform>` must be either `linux/amd64` (x86_64) or `linux/arm64`

These builds include the `mimalloc` feature. The Github Actions builds are made with buildx and are identical to the builds produced in the `runtime-debian` image.

The `scripting` feature cannot be used in musl builds.

## Configuring

Upon running the server for the first time, a `config.toml` file is created. TODO rest of this