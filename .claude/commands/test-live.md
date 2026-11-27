---
description: Run all tests, including live edit tests against Valkey and a real Redis
---
1. Start a scratch Valkey: `valkey-server --port 6391 --save "" --appendonly no --daemonize yes`.
2. Run `CACHE_DECK_TEST_PORT=6391 cargo test` and report the result.
3. If Docker is running, also test real Redis (Homebrew's `redis-server` is Valkey):
   `docker run -d --rm --name cachedeck-redis-test -p 127.0.0.1:6393:6379 redis:7-alpine`, then `CACHE_DECK_TEST_PORT=6393 cargo test live`, then `docker stop cachedeck-redis-test`.
4. Shut the scratch Valkey down: `valkey-cli -p 6391 SHUTDOWN NOSAVE`.
