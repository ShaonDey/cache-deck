#!/bin/sh
# Build "Cache Deck.app" into dist/. Run from anywhere:
#   sh packaging/macos/bundle.sh               for this Mac's own chip
#   sh packaging/macos/bundle.sh --universal   one app for Apple Silicon and Intel Macs
# --universal needs both Rust targets once:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "${1:-}" = "--universal" ]; then
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    BIN="target/universal-apple-darwin/release/cache-deck"
    mkdir -p "$(dirname "$BIN")"
    lipo -create -output "$BIN" \
        target/aarch64-apple-darwin/release/cache-deck \
        target/x86_64-apple-darwin/release/cache-deck
else
    cargo build --release
    BIN="target/release/cache-deck"
fi
APP="dist/Cache Deck.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Cache Deck"
cp assets/app-icon/icon.icns "$APP/Contents/Resources/AppIcon.icns"
cp packaging/macos/Credits.html "$APP/Contents/Resources/Credits.html"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist.in > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
codesign --force --deep --sign - "$APP" 2>/dev/null || echo "note: could not sign (the app still runs on this Mac)"
echo "Built $APP (version $VERSION)"
