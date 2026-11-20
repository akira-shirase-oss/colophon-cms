# D-223 notes: write-path capacity on the production shape

Working notes for the open decision D-223 (`docs/DECISIONS.md`). The production shape is a 2-vCPU droplet
running only the server, with Postgres on Neon, remote and `sslmode=verify-full`: every statement inside a
transaction is a round trip, so a lock costs (round trips inside it) × RTT.

## Found so far

Statement logs on a local 2-CPU stack (a laptop development observation), counting the round trips each
lock is held across:

| Path | Lock | Round trips under it | Ceiling it implies (an inference) |
|---|---|---:|---|
| A member change (role, removal) | every owner row of the project, `FOR UPDATE` | 4 | ≈ 1/(4 × RTT) per project, by design (the last-owner rule) |
| An upload, before D-216 | installation-wide `WB` | 8 | ≈ 1/(8 × RTT) for the whole installation |
| An upload, D-216 | installation-wide counter row | 3 | ≈ 1/(3 × RTT) for the whole installation |
| An upload, D-216 | per-project `WA` | 7 | ≈ 1/(7 × RTT) per project |
| Any transaction's release (D-217) | none: one extra statement | +1 | one more round trip of pool occupancy per transaction (D-226 makes it 1 in 256) |

CI 44673105548 showed the members queue becoming a convoy on a fast runner, with no server change: p99
1,314 ms, 236 slow `lock_involved` `FOR UPDATE` (≤148 ms), 218 of them in one 3-second burst, the disk calm.

## The pool-drain path

A request cancelled mid-statement (a deadline, a client that went away) leaves its statement running at the
server, and its connection isn't pooled until the statement ends: sqlx's release `ping` waits for the
statement's answer. In D-226's path tests a dropped handler running `pg_sleep(30)` held its connection for
30.9 s. Slow queries plus impatient clients therefore drain the pool, bounded only by `statement_timeout`:
D-225's per-pool timeout (reads 2 s) is the bound that matters.

Cancelling at the server when the request is dropped would return the connection at once, but sqlx 0.9.0
offers no way to: the backend's cancel key is read at connect and kept private. Working around it (learning
each connection's backend pid at connect, then `pg_cancel_backend(pid)` from another connection) has nowhere
to keep the pid with its connection, and needs a free connection exactly when the pool is draining, so it
isn't pursued. The fix belongs upstream.

## Draft: an issue for sqlx (not filed)

Filing it is an outward action from the maintainer's account, so it waits for the maintainer's yes.

> **Title:** Expose a cancel handle for `PgConnection` (send a CancelRequest for an in-flight query)
>
> **Problem.** When a future running a query on a `PgConnection` is dropped (a request deadline, a client
> that disconnected), the query keeps running on the server. The connection can't be reused until it ends:
> on release the pool waits for the pending `ReadyForQuery`. Under slow queries and impatient clients this
> drains the pool, and `statement_timeout` is the only bound. Postgres has a protocol answer for exactly
> this, the CancelRequest sent on a separate connection with the backend's process id and secret key, but
> sqlx gives users no way to send one.
>
> **Where it stands in 0.9.0.** sqlx-postgres already reads `BackendKeyData` during startup
> (`connection/establish.rs`) and stores both values in `PgConnectionInner { process_id, secret_key }`
> (`connection/mod.rs`). Neither is reachable: there's no accessor and no cancel API. (The public
> `PgNotification::process_id()` is the *notifying* backend's pid, not the connection's.)
>
> **Precedent.** tokio-postgres's `Client::cancel_token()` returns a `CancelToken` that holds the key and
> connect parameters and can be sent later, from anywhere, with `CancelToken::cancel_query(tls)`; libpq has
> `PQgetCancel` / `PQcancel`.
>
> **Proposal.** `PgConnection::cancel_token(&self) -> PgCancelToken` (cloneable, `Send + Sync`, holding the
> process id, the secret key and the connect options needed to reach the same server, TLS included) and
> `PgCancelToken::cancel(&self) -> impl Future<Output = Result<(), Error>>`, which opens a new connection,
> sends the CancelRequest and closes. The caller takes the token before running a query and sends it from
> its drop or timeout path. Optionally later: a pool-level option to cancel automatically when a
> connection is released with a query still in flight.
>
> **Alternatives considered.** Recording `pg_backend_pid()` per connection and calling `pg_cancel_backend`
> from another pooled connection needs per-connection storage sqlx doesn't offer, and a free connection at
> exactly the moment the pool is exhausted.
