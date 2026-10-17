#!/bin/sh
# Build a terminal-independent, ad-hoc signed macOS menu bar application.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
cargo build --release
app="${1:-${root}/target/Tyler.app}"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp macos/Info.plist "$app/Contents/Info.plist"
cp target/release/tyler-tiler "$app/Contents/MacOS/tyler-tiler"
swiftc -O -swift-version 5 -module-cache-path "$root/target/swift-module-cache" \
  -framework AppKit -framework ApplicationServices macos/Tyler.swift \
  -o "$app/Contents/MacOS/Tyler"
codesign --force --sign - --identifier com.ndelah.tyler.worker "$app/Contents/MacOS/tyler-tiler"
codesign --force --sign - --identifier com.ndelah.tyler "$app"
echo "Built $app"
