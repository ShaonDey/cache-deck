# Security

## Reporting a problem

Please report security problems **privately**, not in a public issue:

1. Open the repository's **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe what you found and how to reproduce it.

Or use this link: https://github.com/ShaonDey/cache-deck/security/advisories/new

Cache Deck is maintained by one person in their spare time, so replies can take a few days.

## Supported versions

Only the latest release gets fixes.

## What counts

Cache Deck is a desktop client that connects to Redis and Valkey servers you choose. Reports that are in scope include:

- a server, a key or a value that can make Cache Deck run code, read files or send data somewhere else
- TLS problems, such as a certificate being accepted when it should not be
- saved passwords ending up somewhere they should not (logs, screenshots, other users' files)
- problems in the release files or in the build workflows

## Known and intended

- Saved passwords are stored as plain text in the settings file, readable only by your user account on macOS and Linux. This is documented in the README. Using the system keychain instead is a planned improvement, not a vulnerability report.
- "Skip the certificate host name check" is an option you turn on per connection, for tunnels. The certificate still has to come from a trusted authority.
- The release files are not signed with a paid certificate, so Windows and macOS show a warning on first start.
