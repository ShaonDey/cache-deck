#!/bin/sh
# Build "Cache Deck.app" into dist/. Run from anywhere: sh packaging/macos/bundle.sh
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cargo build --release
APP="dist/Cache Deck.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/cache-deck "$APP/Contents/MacOS/Cache Deck"
cp assets/app-icon/icon.icns "$APP/Contents/Resources/AppIcon.icns"
cp packaging/macos/Credits.html "$APP/Contents/Resources/Credits.html"
sed "s/@VERSION@/$VERSION/g" packaging/macos/Info.plist.in > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
plutil -lint "$APP/Contents/Info.plist" >/dev/null
codesign --force --deep --sign - "$APP" 2>/dev/null || echo "note: could not sign (the app still runs on this Mac)"
echo "Built $APP (version $VERSION)"
