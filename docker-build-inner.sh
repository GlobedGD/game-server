mv .cargo .cargo-temp
rustup toolchain add nightly
rustup override set nightly
RUSTFLAGS="--cfg tokio_unstable" SERVER_SHARED_PREBUILT_DATA=1 cargo build --release --no-default-features --features tokio_tracing
mv .cargo-temp .cargo
cp target/release/game-server /work/game-server-x64