#!/bin/sh
# Build the Mac disk image: dist/cache-deck-<version>-macos-universal.dmg
# It opens with "Cache Deck.app" next to a shortcut to Applications, to drag one onto
# the other. Run from anywhere: sh packaging/macos/dmg.sh
# Needs both Rust targets once: rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -eu
cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
sh packaging/macos/bundle.sh --universal

STAGE="dist/dmg"
DMG="dist/cache-deck-$VERSION-macos-universal.dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "dist/Cache Deck.app" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

# hdiutil now and then fails with "Resource busy" on build machines; a retry gets past it.
TRY=1
until hdiutil create -volname "Cache Deck" -srcfolder "$STAGE" -ov -format UDZO "$DMG"; do
    if [ "$TRY" -ge 3 ]; then
        echo "hdiutil failed $TRY times" >&2
        exit 1
    fi
    TRY=$((TRY + 1))
    sleep 3
done
rm -rf "$STAGE"
echo "Built $DMG"
