#!/bin/sh
# Build a terminal-independent macOS menu bar application.
# TYLER_SIGN_IDENTITY selects a local certificate; otherwise sign ad hoc.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
cargo build --release
app="${1:-${root}/target/Tyler.app}"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/Info.plist"
cp target/release/tyler-tiler "$app/Contents/MacOS/tyler-tiler"
cp macos/Resources/TylerIcon.png macos/Resources/TylerMenuBar.pdf "$app/Contents/Resources/"
iconset="$root/target/Tyler.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" macos/Resources/TylerIcon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" macos/Resources/TylerIcon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Tyler.icns"
swiftc -O -swift-version 5 -target "$(uname -m)-apple-macosx12.0" -module-cache-path "$root/target/swift-module-cache" \
  -framework AppKit -framework ApplicationServices macos/Tyler.swift \
  -o "$app/Contents/MacOS/Tyler"
signing_identity="${TYLER_SIGN_IDENTITY:--}"
codesign --force --sign "$signing_identity" --identifier com.ndelah.tyler.worker "$app/Contents/MacOS/tyler-tiler"
codesign --force --sign "$signing_identity" --identifier com.ndelah.tyler "$app"
echo "Built $app"
