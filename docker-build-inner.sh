mv .cargo .cargo-temp
rustup toolchain add nightly
rustup override set nightly
SERVER_SHARED_PREBUILT_DATA=1 cargo build --release --features scripting
mv .cargo-temp .cargo
cp target/release/game-server /work/game-server-x64