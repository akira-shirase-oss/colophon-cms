# Colophon's attack surface

Every parser of outside input, and every attack class that applies, with where the defence is and what the
property tests, the fuzz targets (`fuzz/`) and the attack suite (`crates/colophon-server/tests/attacks/`) check
(D-231). Surveyed on 20 November 2026. "Gap" marks a place where the defence was missing or weaker than the rest;
each one was confirmed by a test that failed before its fix, and the section on what was done says how it closed.
Read this before every white-hat review: a new parser or route belongs on this list, with its tests.

## Parsers of outside input

**Rich text.** `rich_text::validate` takes a JSON document and refuses anything outside the format: depth 16
(checked before recursing), 10,000 nodes, 256 KiB canonical, 200 images, alt text up to 300 characters, links
up to 2,048 characters and only `http`, `https`, `mailto`, paths and fragments (`is_allowed_href`). It is pure
and unauthenticated over `POST /api/v1/rich-text/validate`. Gap: the 256 KiB limit is checked after the
canonical string is built, so a 21 MiB body of control characters grows to about six times its size first.
Rust never renders rich text to HTML; `@colophon/content-renderer` does, re-checking every node and escaping
text and attributes. Its output goes into `{@html}` and `set:html` in the Studio and the example sites, so the
renderer is the stored-XSS boundary.

**Content types and entry JSON.** `ContentType::parse` checks a definition (100 fields a level, structures 3
deep, 20 block kinds, 100 enum options, patterns up to 500 characters compiled with a 1 MiB regex limit);
`ContentType::validate` checks an entry against it (text up to `max_chars`, lists and references up to 100,
JSON fields 64 KiB and 32 deep, numbers as safe integers or 6-place decimals). Both are pure. Gap: a field's
`when.is` list is cloned and checked for duplicates in O(n²) before its length (100) is checked, so one
content-type write of a few MiB holds a transaction for minutes of CPU. `expand_components` and
`components_used` walk the raw definition before the depth check, bounded only by serde_json's recursion limit
of 128.

**Queries and GraphQL.** `Query::validate` checks a query document (8 filters, 4 sort keys, 100 `in` values,
limit 1–100, reference depth 2) and the opaque cursor (length before decoding, 8 KiB after). The per-collection
GraphQL endpoint refuses queries over 8 KiB, nesting over 64, depth over 16, complexity over 512, more than 8
root reads or 128 selections a subtree, and fragment cycles; it compiles to the same query document. SQL is
built from fixed text and closed enums with every value and field name bound, so injection through a filter
has no path; a range filter's literal is cast in SQL, so a value the domain doesn't type-check could surface as
a database error rather than a 400 (to fuzz). Introspection is on and shows only collections the caller may
read. Four hand-written base64 cursors in delivery, projects and preview are private and bounded by URI length.

**Imports, migrations and bulk.** `import_items` reads a JSON array, a Strapi `{data, meta}` envelope or JSON
Lines, then `transfer::map` maps each item onto the type. Gap: the whole body (up to 21 MiB) is parsed into
values, inside the write lock and transaction, before the 512-item limit is checked. `migrate::parse` and
`bulk::parse` read field operations (paths up to 9 segments; bulk 100 items and 1 MiB a chunk, 10,000 in all).
Gap: `migrate::apply` panics on an operation with an empty path (`path.len() - 1`); `parse` never produces
one, but the operation type is public.

**Uploads and media.** Sign-in and permission are settled before a byte is read; then the declared length, a
shared memory budget (503), four uploads at once per credential (429), 15 s idle (408) and a minimum rate.
Only PNG, JPEG and WebP are accepted, up to 20 MiB; the worker sniffs magic bytes, decodes with
16,384-pixel and 256 MiB caps and 25 megapixels, and re-encodes to strip metadata. SVG is refused. File names
are display text only: storage keys are built from UUIDs, so there is no path to traverse. Media is served
with `nosniff` and the stored type.

**Destinations and previews.** A destination URL must be https without credentials, and an IP-literal host
must be public; the worker's resolver keeps only public addresses (no DNS rebinding), follows no redirects and
uses no proxy. Gap: the IPv6 check doesn't refuse IPv4-compatible `::a.b.c.d`, 6to4 `2002::/16`, Teredo
`2001::/32`, site-local `fec0::/10` or `64:ff9b:1::/48`, which can embed a private IPv4 address. Preview
templates are https only, up to 2,048 characters, with five placeholders kept out of the host; the server
never fetches them. Outgoing webhooks are not signed and carry no timestamp; there is no incoming webhook.

**MCP.** `POST {project}/mcp` takes a project API token only, an exact trusted `Origin` when one is sent, the
pinned protocol version and messages up to 2 MiB. Tool inputs are typed structs with `deny_unknown_fields`,
checked by small helpers (slugs, UUIDs, cursors, hashes, comments up to 8,000 characters). There is no publish,
approve or schedule tool, and every call is forwarded to the API with the same token. Calls are dispatched
in-process without the HTTP middleware (body limit, deadline); they are bounded by a 15 s attempt timeout and a
4 MiB answer instead.

**Receipts, locales, schedules and the small parsers.** Receipts are read-only and not signed; the only
outside input is the probe answer the worker reads (`receipt::marker`, up to 18 digits, body capped at 1 MiB).
`locale::canonical`, `Visibility::parse`, `preview::Template::parse` and `fill`, and `destination::check_url`
are pure string parsers. `schedule::check_window` and `delayed` subtract unchecked `i64`s (debug overflow on
extreme values; the API only passes `jiff` times). The OIDC callback accepts only asymmetric signatures and
checks `iss`, `aud`, `exp`, `nbf`, `azp` and the nonce.

## Attack classes

**Cross-project access and IDOR on entries, assets, releases and the rest.** Every content route opens a
transaction scoped by the path's project UUID; row-level security is forced and keyed on the transaction's
project and identity, and the app login cannot bypass it. A token on another project's path gets 404. No
handler loads a row by an attacker-chosen id alone. Gap (closed by D-229): `/media/v1/{project}/{env}/{asset}` was
public with no signature, the asset need not be published, and asset ids are UUIDv7 (the leading part is a
timestamp).

**Token scope escalation.** Granting needs `tokens.manage` read fresh and a grant the granter covers; no token
holds members, roles, tokens, audit or audience management; tokens are path-confined to their project's
`environments/` routes and can't schedule, undo or approve another token's changeset. Gap (closed by D-228): a
project API token kept its permissions after its creator was demoted or removed; nothing re-checked the
creator.

**Sessions and CSRF.** Session cookies are `HttpOnly; Secure; SameSite=Lax`; every cookie-authenticated write
needs an exact trusted `Origin` and a matching `X-CSRF-Token`; OIDC state, device codes, invitations, setup
codes, magic links and preview codes are single-use with expiry (see replay). Sign-in and sign-up are JSON
only, so a cross-site form should be refused (to test with `text/plain`).

**Header spoofing.** The client address is the TCP peer; `X-Forwarded-For` is read only when the peer is in
`COLOPHON_TRUSTED_PROXIES`, right to left. `X-Real-IP`, `CF-Connecting-IP`, `Forwarded` and `Host` are never
read; link origins come from configuration. Only a unit test covers a spoofed header from an untrusted peer.

**Rate-limit evasion.** GCRA in Valkey, per address and per credential, with a stricter class for password
work and a per-token quota from Postgres; each server falls back to its own limits without Valkey. Gaps:
`OPTIONS /preview/…` with an `Origin` does a database read in the CORS layer, which sits outside the limiter;
`/me` (a database read with a cookie) and `/media/v1/*` aren't limited; the local fallback map clears entirely
at 100,000 keys, so a flood of addresses while Valkey is down resets everyone. A random bearer gets a fresh
credential key, so only the per-address limit holds it (by design).

**SSRF.** Covered under destinations: https only, public addresses only at save, before each send and at the
resolver, no redirects. Gap: the IPv6 ranges above. The local config allows private destinations, so the
attack tests start the server without that switch.

**Oversized and slow bodies.** A declared length over the limit gets 413 before any handler; a streamed body
is cut at the limit; unauthenticated forms are held to 16 KiB and preview exchange to 1 KiB; the whole
request, body included, has a 10 s deadline except uploads, which have their own. Gaps (closed by D-230): signed-in JSON routes
accepted the full 21 MiB upload limit; no header-read timeout was set on the listener, so slow headers held a
connection.

**Replay of signed things.** Nothing in Colophon carries a signature an outsider can replay: receipts and
webhooks are unsigned, and tokens are random bearers looked up by hash. One-time things are spent in one
statement (`DELETE … RETURNING` or a conditional `UPDATE`): OIDC state, device codes, setup codes, magic links
(using one retires its siblings) and preview codes. A preview token is a reusable bearer until its grant ends,
re-checked against the issuer on every read. Idempotency keys are bound to the caller. To test: an invitation
whose acceptance rolled back with `AlreadyMember` stays usable.

**SQL and GraphQL injection.** Covered under queries: no value is spliced into SQL text; the GraphQL schema is
built per request from the caller's readable collections. The suite sends injection strings through filters,
sorts, field names and GraphQL variables and asserts a 400 or an empty result, never a 500.

**Stored XSS through rich text and Markdown.** The Rust validator refuses unknown nodes, attributes and
schemes; the renderer re-checks and escapes, and `renderMarkdown` escapes each paragraph before its inline
rules. A differential test renders fuzzed documents and parses the HTML to assert no script, no event
handler and no `javascript:` URL.

## What was done

| Gap | Fixed by | Test that failed first |
|---|---|---|
| A: `when.is` checked for duplicates before its length | the length first (D-231) | `a_huge_condition_list_is_refused_at_once` |
| B: `migrate::apply` on an empty path | a refusal (D-231) | `an_operation_with_an_empty_path_is_refused` |
| C: schedule windows overflow on extreme instants | saturating arithmetic (D-231) | `schedule_windows_answer_any_instants` |
| D: IPv6 destinations embedding private IPv4 | judged by the embedded address; Teredo, site-local, NAT64, discard-only refused (D-231) | `private_addresses_inside_ipv6_are_refused`, attack `ssrf` |
| E: import parsed whole, under the write lock, before its 512-item check | counted while read, before the lock (D-231) | `an_import_over_the_limit_is_refused_while_it_is_read` |
| F: rich text built six times its size before the 256 KiB check | refused before the text is written (D-231) | `oversized_text_is_refused_before_it_is_written` |
| G: preview preflight's database read outside the rate limits; `/me` and media unlimited | the CORS layer inside the limiter; `/me` and media classed (D-231) | attack `limits` |
| A token outlives its creator's demotion or removal | the token does what both it and the creator's current role allow; removal revokes it (D-228) | `a_token_is_held_to_its_creator_s_role_as_it_is_now`, attack `tokens` |
| Unpublished media served to anyone with its id | private until a release names it; members and signed addresses only (D-229) | attack `media` |
| No header-read timeout; 21 MiB JSON on every signed-in route | a header deadline on the listener, 1 MiB JSON unless documented (D-230) | attack `bodies` |

Checked and found to hold, now with a regression test each: cross-project reads and writes and token path
tricks (`tenancy`), login CSRF through a cross-site form and cookie writes without origin or CSRF token
(`sessions`), hostile query and GraphQL input (`injection`), script in rich text (`sessions`), a forged
`X-Forwarded-For` behind the trusted proxy (`limits`), and an invitation's single use after a member tried it
(`replay`: refused without spending it, by design, so the person it was sent to can still join, once).

Not built here, and why: outgoing webhooks are unsigned (nothing receives them yet that checks a signature; a
signing scheme is a decision of its own); MCP tool calls skip the HTTP body and deadline layers, bounded by their
own 15 s attempt and 4 MiB answer; the local rate-limit fallback clears at 100,000 keys while Valkey is down.

## How it runs

1. **Never-panics property tests** in `crates/colophon-domain/tests/never_panics.rs` (stable, part of
   `cargo test`): every parser above, seeded from `fixtures/rich-text/v1` and hand-written cases, with round-trip
   properties where they hold (cursor, visibility, locale, canonical forms, a preview address's origin).
2. **cargo-fuzz targets** in `fuzz/`, outside the workspace with its own lockfile, one per parser family, each
   with a seed corpus in `fuzz/corpus/<target>`; CI's fuzz job runs every target for 60 s, with a 2 s limit per
   input so a stall fails like a crash.
3. **The attack suite**, `crates/colophon-server/tests/attacks/`: real requests against the running stack, one
   module per attack class, each asserting the refusal; it runs in CI's integration job with the other stack
   tests.
