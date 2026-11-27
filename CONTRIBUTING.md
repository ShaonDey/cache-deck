# Contributing to Cache Deck

Thanks for wanting to help. Bug reports, ideas and pull requests are all welcome.

## Reporting bugs and suggesting features

Open an issue and pick the matching form. Please leave out passwords, host names and key contents from real servers.

Security problems go through the private route described in [SECURITY.md](SECURITY.md), not through issues.

## Proposing a change

1. For anything bigger than a small fix, open an issue first, so we can agree on the approach before you spend time on it.
2. Fork the repository and create a branch from `main`.
3. Make the change. Keep it to one topic per pull request.
4. Run the checks below.
5. Open a pull request against `main` and fill in the template.

Pull requests are checked automatically (formatting, lints, and tests on Windows, Linux and macOS). The first time you contribute, the owner has to approve that run, so it may not start right away.

## Building and checking

You need [Rust](https://rustup.rs) (stable). On Linux you also need the window-system development libraries listed in the README.

```sh
cargo run                          # start the app
cargo fmt --check                  # formatting (cargo fmt fixes it)
cargo clippy -- -D warnings        # lints, must be clean
cargo test                         # unit tests
```

To run the live edit tests as well, start a scratch server and point the tests at it. They use database 5 and **empty it**, so never point them at a server you care about:

```sh
valkey-server --port 6391 --save "" --daemonize yes
CACHE_DECK_TEST_PORT=6391 cargo test
```

After a change to the screens, take a picture and look at it. Use a throwaway settings file so your own saved connections are not touched:

```sh
CACHE_DECK_CONFIG=/tmp/cache-deck-test.json ./target/debug/cache-deck --screenshot shots/now.png
```

## Ground rules for the code

- **Never block the window with network calls.** Talk to servers through the worker (`Job` in, `Msg` out) or a spawned thread.
- **Keys are bytes.** Convert to text only for display.
- **Deleting asks first.** Anything that deletes a key, field, member or connection goes through the confirm dialog.
- **Colours come from `Palette`** in `src/theme.rs`. Red is reserved for errors, deletes and the PROD tag.
- **TLS stays strict.** The certificate chain is always verified. Don't add a way to accept any certificate.
- **Code for one system only** goes in `src/platform/`. Packaging files go in `packaging/<system>/`.
- **Changes to what is sent to a server** should be tried against both Valkey and Redis.
- **No Redis or Valkey logos**, and no red-key imagery. The icon is the project's own.
- The visible name is always "Cache Deck"; the program and crate name is `cache-deck`.

`CLAUDE.md` holds more detail on the layout and on pitfalls with the UI toolkit. It is written for AI coding tools but is just as useful to people.

## Licence

By contributing, you agree that your contribution is released under the project's MIT licence.
