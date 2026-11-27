# Cache Deck: notes for Claude and contributors

Cache Deck is a desktop client for Redis and Valkey, written in Rust with eframe/egui 0.37 and redis-rs 1.x (sync API).

## Commands

```sh
cargo build                         # debug build
cargo run -- --connect 127.0.0.1:6379
cargo test                          # unit tests
CACHE_DECK_TEST_PORT=6391 cargo test   # also live edit tests against a scratch server (uses db 5, FLUSHDB)
cargo clippy && cargo fmt           # must be clean; rustfmt.toml sets max_width = 120
sh packaging/macos/bundle.sh        # dist/Cache Deck.app (release build, ad-hoc signed)
sh packaging/macos/dmg.sh           # dist/cache-deck-<version>-macos-universal.dmg (needs both Mac Rust targets)
cargo deb                           # Linux only: target/debian/*.deb (needs `cargo install cargo-deb`)
cargo generate-rpm                  # Linux only: target/generate-rpm/*.rpm (needs `cargo install cargo-generate-rpm`)
```

Slash commands in `.claude/commands/`: `/screenshot`, `/test-live`, `/bundle-mac`, `/check`.

## Layout

- `src/main.rs`: options (`--screenshot`, `--connect`, `--select`, `--tab`, `--light`, `--run`), window setup, quiet screenshot mode (no focus steal: `with_active(false)`, macOS `with_activate_ignoring_other_apps(false)`, Windows `AlwaysOnBottom`).
- `src/app.rs`: `CacheDeck`, the whole UI. `Tab::{Dashboard, Keys, Console, Info}`. The dashboard is the landing page and is shown whenever no server is connected. Dialogs (connection editor, confirm, new key, About) are `egui::Modal`s in `dialogs()`.
- `src/worker.rs`: a single background thread owns the active `redis::Connection`. The UI sends `Job`s and receives `Msg`s, polled in `poll()` each frame. Dashboard numbers come from `probe()`, one short-lived connection per server on its own thread. `format_value` renders replies like redis-cli. `split_args` parses typed commands.
- `src/config.rs`: `Config` / `ConnCfg` (with `env` tag and `last_used`), JSON in the user config dir, file mode 0600. `CACHE_DECK_CONFIG` overrides the path.
- `src/theme.rs`: `Palette` (`DARK`, `LIGHT`), `type_color`, `install`. All colours come from here.
- `src/platform/`: code for one system only, one file per system, switched on in `platform/mod.rs`. Shared code stays in the files above. Short `#[cfg(...)]` blocks of a few lines may stay inline where they are used (`main.rs`, `app.rs`, `config.rs`).
- `src/platform/macos.rs`: re-points the app menu's "About Cache Deck" to a standard About panel filled with icon, version, developer and GitHub link (works unbundled too).

## Packaging

Per-system packaging files live in `packaging/<system>/`. The build script is `packaging/windows/build.rs`, not a `build.rs` at the root; `build = ...` in `Cargo.toml` points Cargo at it. `.github/` stays at the root because GitHub only looks there.

- macOS: `packaging/macos/bundle.sh` builds `dist/Cache Deck.app` from `packaging/macos/Info.plist.in` and the `.icns` icon. With `--universal` it builds both chips and merges them with `lipo`. `packaging/macos/dmg.sh` wraps the universal app in `dist/cache-deck-<version>-macos-universal.dmg`.
- Windows installer: `packaging/windows/installer.iss` (Inno Setup, per-user install, no admin prompt). Never change its `AppId`, or an update installs beside the old version instead of replacing it.
- Linux packages: `[package.metadata.deb]` and `[package.metadata.generate-rpm]` in `Cargo.toml` hold the same file list in two syntaxes. Change both together.
- Windows: `packaging/windows/build.rs` puts `assets/app-icon/icon.ico` and the version details into the `.exe` through the `winresource` build dependency. That dependency is only compiled when the build runs on Windows, so the Windows build is never cross-compiled. `main.rs` sets `windows_subsystem = "windows"` for release builds (no console window, so no `--help` output there).
- Linux: `packaging/linux/cache-deck.desktop` is the app-menu entry. Its file name and `StartupWMClass` must stay equal to the `with_app_id("cache-deck")` value in `main.rs`, or the window loses its icon on Wayland. `[package.metadata.deb]` in `Cargo.toml` lists what the `.deb` installs.
- `.github/workflows/release.yml` builds all three on GitHub's machines (Windows x86_64 installer, bare `.exe` and `.zip`; Linux x86_64 and arm64 `.deb`, `.rpm` and `.tar.gz`; macOS universal `.dmg`). A pushed tag `v<version>` must match the `Cargo.toml` version and ends in a draft release; a run by hand only builds. It builds with `--locked`, so commit `Cargo.lock` whenever dependencies change.
- The Windows and Linux builds have not been run on real machines yet. Say so when reporting on them.

## Rules

- TLS needs a crypto backend: `main.rs` installs `ring` at startup through the direct `rustls` dependency. Don't remove either; without them every TLS connection panics (the `redis` crate turns on rustls with no backend). `worker::connect` also turns a panic into an error so a spinner can't hang.
- TLS always verifies the certificate chain. `ConnCfg::tls_skip_host_check` (off by default) only skips comparing the certificate's name with the host, for tunnels through 127.0.0.1; it uses `set_danger_accept_invalid_hostnames`. Never set `insecure: true` on `ConnectionAddr::TcpTls`: that accepts any certificate.
- Never block the UI thread with network calls. Go through `Worker` (`Job` → `Msg`) or a spawned thread that sends a `Msg` and calls `ctx.request_repaint()`.
- Keep keys as bytes (`Bytes`). Only convert to text for display.
- Anything that deletes data (key, field, member, connection) asks first through the `Confirm` dialog, except removing single items inside a value.
- Colours come only from `Palette`. Red (`p.error`) is reserved for errors, deletes and the PROD tag.
- Visible app name is always "Cache Deck"; the crate and binary name is `cache-deck`.
- Developer details (Shaon Dey, https://github.com/ShaonDey, © 2026) live in `DEVELOPER` / `GITHUB_URL` in `app.rs`, in `src/platform/macos.rs`, in `packaging/macos/`, in `packaging/windows/build.rs`, in `packaging/windows/installer.iss`, in `LICENSE` and in `Cargo.toml` (`authors`, `homepage`, `[package.metadata.deb]`). Don't change them without the owner.
- Test changes that touch server traffic against both Valkey and Redis. Note: Homebrew's `redis-server` is Valkey, so use `docker run -p 127.0.0.1:6393:6379 redis:7-alpine` for real Redis.
- Don't use Redis or Valkey logos or red-key imagery; the icon is our own (assets/app-icon/icon.svg).

## egui 0.37 pitfalls we hit

- API: `egui::Panel::left/right/top/bottom(..).show(ui, ..)`, `App::ui(&mut self, ui, frame)`, `Id::unique(..)` (`Id::new` is deprecated), `TextEdit::align(Align2)`, `CornerRadius` / `Margin` take integers.
- Panels grow to fit their content. In a row, add fixed-size widgets first (`right_to_left`) and give text boxes `ui.available_width()`, or the panel creeps wider every frame.
- `item_spacing` set in a parent `ui.horizontal` is inherited by everything inside, including cards. Reset it inside the card.
- Widgets in a `ui.horizontal` inside a `Frame` lay out sideways. Wrap card contents in `ui.vertical`.
- Fixed-width table cells: use the `cell()` helper (truncates with "…" and shows a hover tooltip).

## Checking the UI

After a UI change, take a picture and look at it:

```sh
./target/debug/cache-deck --screenshot shots/now.png                       # dashboard
./target/debug/cache-deck --connect 127.0.0.1:6391 --select user:42 --screenshot shots/keys.png
./target/debug/cache-deck --tab new --light --screenshot shots/form.png
```

Use `CACHE_DECK_CONFIG=/tmp/some.json` with sample connections so a real user's settings are never touched.
