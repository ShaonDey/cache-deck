---
description: Build the Mac app package dist/Cache Deck.app
---
Run `sh packaging/macos/bundle.sh`. Then confirm `dist/Cache Deck.app/Contents/Info.plist` has the right version (from Cargo.toml) and that `"dist/Cache Deck.app/Contents/MacOS/Cache Deck" --screenshot shots/bundle.png` saves a picture. Report the result. The package is ad-hoc signed and only runs as-is on this Mac.
