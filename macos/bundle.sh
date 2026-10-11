#!/bin/sh
# Build dist/Glyphdrift.app: a universal (Apple silicon + Intel) release binary, the icon
# and Info.plist, ad-hoc signed. Needs Rust with both macOS targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Pass --native to build for this Mac's architecture only.
set -eu
cd "$(dirname "$0")/.."

export MACOSX_DEPLOYMENT_TARGET=12.0
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
app=dist/Glyphdrift.app
bin=$app/Contents/MacOS/glyphdrift

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

if [ "${1:-}" = "--native" ]; then
    cargo build --release
    cp target/release/glyphdrift "$bin"
else
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --release --target "$t"
    done
    lipo -create -output "$bin" \
        target/aarch64-apple-darwin/release/glyphdrift \
        target/x86_64-apple-darwin/release/glyphdrift
fi

iconset=target/Glyphdrift.iconset
cargo run --quiet --example macos_iconset -- "$iconset"
iconutil -c icns -o "$app/Contents/Resources/Glyphdrift.icns" "$iconset"

sed "s/VERSION/$version/g" macos/Info.plist > "$app/Contents/Info.plist"

# Ad-hoc signature: required on Apple silicon. Not notarized, so Gatekeeper asks on first
# open (right-click > Open, or System Settings > Privacy & Security > Open Anyway).
codesign --force --sign - "$app"

(cd dist && ditto -c -k --keepParent Glyphdrift.app "Glyphdrift-$version-macos.zip")
shasum -a 256 "dist/Glyphdrift-$version-macos.zip"
echo "Built $app ($version)"
