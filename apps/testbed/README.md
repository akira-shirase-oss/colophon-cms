# Colophon testbed

A separate frontend that uses Colophon through its real HTTP API, exactly as a client would: for people trying a feature
by hand, and for Playwright in CI. It is not the Studio. Every feature Colophon ships gets a page or a check here
([D-018](../../docs/DECISIONS.md)).

| Page | What it shows |
|---|---|
| Health | `/readyz` polled every second, each dependency, and a log of state changes |
| API contract | The step 0.3 contract checked from the browser: health answers, the error envelope, request ids |
| Load tests | Every committed `scripts/load.sh` run, failures included |

```sh
scripts/stack.sh up          # the local stack
scripts/testbed.sh           # build everything and run the Playwright suite
scripts/testbed.sh --dev     # or explore by hand on http://127.0.0.1:58100 (start colophon-server on :58000 first)
```

The testbed reaches the server through a same-origin proxy (`/server/*`, configured in `vite.config.ts`), so the server
needs no CORS. The outage tests stop and restart real services of the local stack, so the suite runs one test at a time.

It found its first real bug on day one: the server closed connections silently after leaving a request body unread,
which clients that pool connections saw as resets. See `crates/colophon-api/src/body.rs`.
