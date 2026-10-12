#!/bin/sh
# Build dist/Glyphdrift.app: a universal (Apple silicon + Intel) release binary, the icon
# and Info.plist, ad-hoc signed. The screen saver, Glyphdrift.saver, goes inside it; the app
# installs it from there. Needs Rust with both macOS targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Pass --native to build for this Mac's architecture only.
set -eu
cd "$(dirname "$0")/.."

export MACOSX_DEPLOYMENT_TARGET=12.0
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/Glyphdrift.app
bin=$app/Contents/MacOS/glyphdrift
saver=$app/Contents/Resources/Glyphdrift.saver
saver_bin=$saver/Contents/MacOS/Glyphdrift

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$saver/Contents/MacOS"

if [ "${1:-}" = "--native" ]; then
    cargo build --locked --release -p glyphdrift -p glyphdrift-saver
    cp target/release/glyphdrift "$bin"
    cp target/release/libglyphdrift_saver.dylib "$saver_bin"
else
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --locked --release -p glyphdrift -p glyphdrift-saver --target "$t"
    done
    lipo -create -output "$bin" \
        target/aarch64-apple-darwin/release/glyphdrift \
        target/x86_64-apple-darwin/release/glyphdrift
    lipo -create -output "$saver_bin" \
        target/aarch64-apple-darwin/release/libglyphdrift_saver.dylib \
        target/x86_64-apple-darwin/release/libglyphdrift_saver.dylib
fi

iconset=target/Glyphdrift.iconset
cargo run --locked --quiet --example macos_iconset -- "$iconset"
iconutil -c icns -o "$app/Contents/Resources/Glyphdrift.icns" "$iconset"

sed "s/VERSION/$version/g" macos/Info.plist > "$app/Contents/Info.plist"
sed "s/VERSION/$version/g" macos/Saver-Info.plist > "$saver/Contents/Info.plist"

# Ad-hoc signature: required on Apple silicon. Not notarized, so Gatekeeper blocks the first
# launch until the user clicks Open Anyway in System Settings > Privacy & Security.
# The saver is signed first, as code nested in the app.
codesign --force --sign - "$saver"
codesign --force --sign - "$app"

# A fixed name, so releases/latest/download/Glyphdrift-macos.zip always works.
(cd dist && ditto -c -k --keepParent Glyphdrift.app Glyphdrift-macos.zip)
shasum -a 256 dist/Glyphdrift-macos.zip
echo "Built $app ($version)"
