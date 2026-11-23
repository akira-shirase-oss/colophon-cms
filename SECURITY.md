# Security policy

## Supported versions

Colophon is pre-release software. Until 1.0, the **latest release on the `0.1.x` line**
is the only one that receives security fixes; there is no back-port line yet. From 1.0
the policy in `docs/modules/19-oss.md` applies: the current stable minor and the
previous minor are supported, with a proposed 90-day transition, and the compatibility
and support matrix is published in each release.

| Version | Supported |
|---|---|
| `0.1.x` (latest release) | Yes |
| Older pre-releases | No — upgrade first |

Every binary reports its version, commit and build date: `colophon --version`,
`colophon-server --version` or `GET /version`. If a deployment can't say which build it
runs, treat that as the first thing to fix.

## Reporting a vulnerability

**Please report privately through GitHub security advisories:**
[github.com/akira-shirase-oss/colophon-cms/security/advisories/new](https://github.com/akira-shirase-oss/colophon-cms/security/advisories/new)

Do not open a public issue for something that could be exploited. Personal contact
details are deliberately not published (module 19); the advisory thread is the channel,
and it stays private until a fix ships.

A useful report includes: the affected version and commit (the `GET /version` answer),
what you could make happen (read or write across a project boundary, bypass a policy,
exhaust a resource, recover deleted or draft content, forge a delivery), the smallest
reproduction you have, and whether the deployed configuration mattered — Colophon's
authorization, RLS, budgets and outbox are all in scope.

**What to expect:** an acknowledgement within a week (this is a solo-maintainer project,
not a 24/7 service — module 19's honest support policy), a severity assessment, then a
fix, an advisory and a patch release when warranted. Reporters are credited in the
advisory unless they ask not to be.

## Operator notes

- Local-stack credentials in `deploy/` are public development values — **never reuse
  them in production** (`deploy/README.md`). The stack binds every port to `127.0.0.1`.
- Backup secrets live in files, never flags (`deploy/runbooks/backup.md`); a suspected
  leak follows the credential-leak runbook in module 18: revoke the scoped principal,
  rotate related secrets, pause its new actions, preserve the audit evidence, and do
  not claim revocation undid external effects already executed.
- Development affordances are refused in production shapes, but never set them there:
  `COLOPHON_DEV_MAIL=1` hands sign-in links back in responses (it needs a loopback listener
  and a localhost `COLOPHON_PUBLIC_URL`, D-163), and `COLOPHON_ALLOW_PRIVATE_DESTINATIONS=1`
  lets webhooks, purges and probes reach private and loopback addresses (D-149). Both are in
  `deploy/local.env`; don't copy that file into a production environment.
- Behind a reverse proxy, set `COLOPHON_TRUSTED_PROXIES` to the proxy's address or network:
  only then is the client's address believed from `X-Forwarded-For`, and every per-address
  bound (rate limits, sign-in pauses, magic-link shares) counts clients rather than the proxy.
- Rate limits while Valkey is away (D-076, D-238): every server falls back to limits of its
  own, so across a cluster of N servers a client gets up to N times each rate until Valkey is
  back; nothing is ever unlimited, and Valkey being down never refuses a request by itself. Each
  server keeps at most 100,000 keys then, and at that bound lets the least restrictive go
  first, never a key that is actually being limited, so a flood of new keys can't reset anyone's
  limit. Per-address limits can't stop a client that rotates addresses (an IPv6 network has
  more than any limit covers, Valkey up or down): the bound on password guesses against an
  account is the per-account sign-in pause, counted in Postgres (D-154), which a Valkey outage
  doesn't touch, as it doesn't touch budgets (D-113).
- Deliveries never use `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` (D-149): a host whose egress
  must go through a proxy can't send webhooks yet.
- Vulnerabilities in dependencies are watched by daily RustSec/OSV checks and
  `cargo deny` (module 19), triaged by severity.
