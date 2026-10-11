#!/bin/sh
# Regenerate the website's built files after changing animations:
#   site/glyphdrift.wasm          the animations for the live gallery and header (always)
#   site/img/anim-<name>.webp     gallery stills (800 px) and viewer images (1600, 2400 px)
#   site/img/in-use.webp          the Settings window over Cubes
#
# Usage: web/build-assets.sh [name ...]
#   Names are animation config names (cubes, helix, ...); only those images are rebuilt.
#   With no names, every image is. Each image regeneration adds to the git history, so
#   name only the animations that changed.
#
# Needs macOS (the images come from `glyphdrift --screenshots`), Rust with the wasm32 target
# (rustup target add wasm32-unknown-unknown) and cwebp (brew install webp).
set -eu
cd "$(dirname "$0")/.."

command -v cwebp >/dev/null || { echo "cwebp not found: brew install webp" >&2; exit 1; }

cargo build --release
shots=$(mktemp -d)
trap 'rm -rf "$shots"' EXIT
target/release/glyphdrift --screenshots "$shots" >/dev/null

if [ $# -eq 0 ]; then
    names=$(cd "$shots" && ls anim-*.png | sed 's/^anim-//; s/\.png$//')
    in_use=yes
else
    names="$*"
    # The Settings image shows Cubes.
    in_use=no
    for n in $names; do [ "$n" = cubes ] && in_use=yes; done
fi

for n in $names; do
    f="$shots/anim-$n.png"
    [ -f "$f" ] || { echo "unknown animation: $n" >&2; exit 1; }
    cwebp -quiet -q 80 -m 6 -resize 800 500 "$f" -o "site/img/anim-$n.webp"
    cwebp -quiet -q 80 -m 6 -resize 1600 1000 "$f" -o "site/img/anim-$n-1600.webp"
    cwebp -quiet -q 80 -m 6 "$f" -o "site/img/anim-$n-2400.webp"
    echo "site/img/anim-$n*.webp"
done
if [ "$in_use" = yes ]; then
    cwebp -quiet -q 82 -m 6 "$shots/in-use.png" -o site/img/in-use.webp
    echo "site/img/in-use.webp"
fi

web/build.sh
