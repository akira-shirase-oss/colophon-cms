# What to create before the deploy (S0)

Everything here is created in your own accounts. **No secret is ever pasted into chat, an issue or
git**: each one is typed or pasted only on the droplet, into the file `DEPLOY.md` names, which only
root can read.

| # | Where | Create | Note for the deploy |
|---|---|---|---|
| 1 | DigitalOcean | A droplet: Ubuntu 24.04 LTS (x64), **2 GB / 1 vCPU** Basic, region **Toronto (TOR1)**, your SSH key, monitoring on, backups optional | Its public IPv4 |
| 2 | DigitalOcean | A cloud firewall on it: inbound **22** from your own address only (the same rule as ufw on the droplet: keep both in step when your address changes, DEPLOY.md step 1), **443** from Cloudflare only (`cloudflare-ips.sh --print` prints the ranges); no other inbound | — |
| 3 | Neon | A project in the region nearest Toronto (AWS us-east-2, Ohio, or us-east-1), Postgres **18** (Colophon is tested on 18 only: if Neon offers only 17, stop and say so before creating), database `colophon`, and its **owner** role | The **direct** (not pooled) connection string — it must not contain `-pooler` |
| 4 | Neon | Point-in-time restore window: the plan's maximum (7 days on Launch); compute set to **not** suspend if you want no cold starts | — |
| 5 | Cloudflare R2 | Two buckets: `colophon-media` (uploads) and `colophon-backups` (ledger exports), both private, location hint **ENAM** | Your account id |
| 6 | Cloudflare R2 | An API token, **Object Read & Write**, limited to those two buckets | Its access key id and secret access key |
| 7 | Cloudflare DNS | `app.colophon.404human.dev` → the droplet's IPv4, **proxied** (orange cloud) | — |
| 8 | Cloudflare SSL/TLS | Mode **Full (strict)**; an **Origin Certificate** for `app.colophon.404human.dev` (RSA, 15 years); and **Authenticated Origin Pulls** on, zone-level (SSL/TLS → Origin Server). Cloudflare's addresses are shared by all its customers, so the origin also requires Cloudflare's client certificate for this zone | The certificate and its private key (pasted on the droplet only) |
| 9 | Cloudflare | Web Analytics **off** for this hostname (it injects a script the CSP blocks) | — |
| 10 | Email (optional for S0) | A Resend API key and a verified sender domain, only if you want sign-in links by email | The key |

What you'll paste on the droplet, and where (DEPLOY.md walks you through each):

- the Neon owner connection string → `/etc/colophon/credentials/migration_database_url`;
- the R2 secret access key → `/etc/colophon/credentials/s3_secret_key`;
- the origin certificate and key → `/etc/caddy/origin.pem` and `/etc/caddy/origin.key`;
- the R2 account id and access key id (not secret) → `/etc/colophon/colophon.env`.

The app's own database password and the cache password are generated on the droplet; you never
see them. One thing generated there you **must** copy out: the backup age key's private line
(DEPLOY.md step 10), into a password manager or onto paper. The logical backups are unreadable
without it.
