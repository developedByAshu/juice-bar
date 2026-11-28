#!/bin/sh
# Builds target/mac/JuiceBar.app for Apple Silicon. Run from anywhere.
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
# The release workflow runs its own tests, because some here need a Mac with real network traffic.
[ -n "$SKIP_TESTS" ] || cargo test --locked -p juicebar-shared
cargo build --locked --release --target aarch64-apple-darwin -p juicebar-mac --bin JuiceBar
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="target/mac/JuiceBar.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/aarch64-apple-darwin/release/JuiceBar "$app/Contents/MacOS/JuiceBar"
sed "s/VERSION_PLACEHOLDER/$version/g" mac/Info.plist > "$app/Contents/Info.plist"
if [ -f shared/assets/juicebar.png ]; then
    icons="$(mktemp -d)/JuiceBar.iconset"
    mkdir -p "$icons"
    for size in 16 32 128; do
        sips -z "$size" "$size" shared/assets/juicebar.png --out "$icons/icon_${size}x${size}.png" > /dev/null
        double=$((size * 2))
        sips -z "$double" "$double" shared/assets/juicebar.png --out "$icons/icon_${size}x${size}@2x.png" > /dev/null
    done
    iconutil -c icns "$icons" -o "$app/Contents/Resources/JuiceBar.icns"
fi
# Signs with the owner's certificate (MAC_SIGN_IDENTITY, default "developedByAshu Code Signing") when
# this Mac has it, with the hardened runtime. The requirement names the bundle id and that exact
# certificate, so macOS permissions (Input Monitoring) survive rebuilds and the updater installs only
# a copy signed the same way. It is self-signed, so it is listed without -v. Without it the app is
# signed ad hoc, still with the hardened runtime, and on the bundle id alone.
bundle_id="com.developedbyashu.juicebar"
identity="${MAC_SIGN_IDENTITY:-developedByAshu Code Signing}"
cert_sha="$(security find-identity -p codesigning | grep -F "\"$identity\"" | head -1 | awk '{print $2}' | tr 'A-F' 'a-f')"
if [ -n "$cert_sha" ]; then
    sign_with="$identity"
    requirement="designated => identifier \"$bundle_id\" and certificate leaf = H\"$cert_sha\""
else
    echo "WARNING: signing identity \"$identity\" not found, signing ad hoc. A copy signed with the certificate will not take updates from this build." >&2
    sign_with="-"
    requirement="designated => identifier \"$bundle_id\""
fi
# Nothing is nested (no helpers or frameworks); the main program is signed with the app.
codesign --force --options runtime --sign "$sign_with" -r="$requirement" "$app"
codesign --verify --deep --strict "$app"
echo "Built $root/$app (signed: $sign_with)"
