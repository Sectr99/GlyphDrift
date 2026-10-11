#!/bin/sh
# Build the animations to WebAssembly for the website: site/glyphdrift.wasm, loaded by
# site/live.js. Needs `rustup target add wasm32-unknown-unknown`. Commit the result:
# the site is deployed as static files, with no build step.
set -eu
cd "$(dirname "$0")/.."
cargo build -p glyphdrift-web --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/glyphdrift_web.wasm site/glyphdrift.wasm
ls -l site/glyphdrift.wasm
