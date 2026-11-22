#!/bin/sh
# Builds target/mac/JuiceBar.app for Apple Silicon. Run from anywhere.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
# The release workflow runs its own tests, because some here need a Mac with real network traffic.
[ -n "$SKIP_TESTS" ] || cargo test -p juicebar-shared -p juicebar-mac
cargo build --release --target aarch64-apple-darwin -p juicebar-mac --bin JuiceBar
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="target/mac/JuiceBar.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/aarch64-apple-darwin/release/JuiceBar "$app/Contents/MacOS/JuiceBar"
sed "s/VERSION_PLACEHOLDER/$version/g" mac/Info.plist > "$app/Contents/Info.plist"
icons="$(mktemp -d)/JuiceBar.iconset"
mkdir -p "$icons"
for size in 16 32 128; do
    sips -z "$size" "$size" shared/assets/juicebar.png --out "$icons/icon_${size}x${size}.png" > /dev/null
    double=$((size * 2))
    sips -z "$double" "$double" shared/assets/juicebar.png --out "$icons/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$icons" -o "$app/Contents/Resources/JuiceBar.icns"
codesign --force --sign - "$app"
echo "Built $root/$app"
