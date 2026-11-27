---
description: Build Cache Deck, take a picture of a screen and look at it
argument-hint: "[extra options, e.g. --tab info --light]"
---
1. Run `cargo build` and fix any errors.
2. Run `./target/debug/cache-deck --screenshot shots/now.png $ARGUMENTS`. If the options include `--connect`, start a scratch server first: `valkey-server --port 6391 --save "" --daemonize yes`.
3. Open `shots/now.png` and describe what's wrong or off compared with the rest of the app (spacing, clipping, text cut off, colours outside `Palette`).
4. Stop any scratch server you started (`valkey-cli -p 6391 SHUTDOWN NOSAVE`).
