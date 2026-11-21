# What to create before the deploy (S0)

Everything here is created in your own accounts. **No secret is ever pasted into chat, an issue or
git**: each one is typed or pasted only on the droplet, into the file `DEPLOY.md` names, which only
root can read.

| # | Where | Create | Note for the deploy |
|---|---|---|---|
| 1 | DigitalOcean | A droplet: Ubuntu 24.04 LTS (x64), **2 GB / 1 vCPU** Basic, region **Toronto (TOR1)**, your SSH key, monitoring on, backups optional | Its public IPv4 |
| 2 | DigitalOcean | A cloud firewall on it: inbound **22** from your own address only (the same rule as ufw on the droplet: keep both in step when your address changes, DEPLOY.md step 1), **443** from Cloudflare only (`cloudflare-ips.sh --print` prints the ranges); no other inbound | — |
| 3 | Neon | A project in the region nearest the droplet (the S0 project is AWS us-east-2, Ohio, for now), Postgres **18** (Colophon is tested on 18 only: if Neon offers only 17, stop and say so before creating), database `colophon`, and its **owner** role. Neon's owner is not a superuser; `init-roles` and `migrate` handle that (D-234, DEPLOY.md step 3) | The connection string as the dashboard gives it: `install.sh` makes a pooled host direct and `sslmode` `verify-full` itself |
| 4 | Neon | Point-in-time restore window: the plan's maximum (7 days on Launch); compute set to **not** suspend if you want no cold starts | — |
| 5 | Cloudflare R2 | Two buckets: `colophon-media` (uploads) and `colophon-backups` (ledger exports and logical dumps), both private, location hint **ENAM**. On `colophon-backups`, **Settings → Bucket lock rules → Add rule**: all objects (empty prefix), retain **7 days**, so a stolen droplet key can't delete the last week of backups; pruning logs those refusals as expected | Your account id |
| 6 | Cloudflare R2 | **Two** API tokens, each **Object Read & Write** on **one** bucket: `colophon-media` for the server, and `colophon-backups` for the backup units. Never one token for both: the server faces the internet, and with the backups' token a break-in could delete every dump past the lock's 7 days. (R2 has no write-only object permission, so the backups' token can read its bucket too; the dumps are encrypted to the offline age key.) If you run one token on both buckets anyway, say so explicitly: `echo yes >/etc/colophon/install/r2_backup_shares_media` before `install.sh`, which then uses it for the backups too and warns on every run (`r2-keys.sh`). | Each token's access key id and secret access key |
| 7 | Cloudflare DNS | `colophonapp.404human.dev` → the droplet's IPv4, **proxied** (orange cloud) | One level under the zone because Cloudflare's free Universal SSL covers only `404human.dev` and `*.404human.dev`: a two-level name such as `app.colophon.404human.dev` fails the TLS handshake at the edge unless you buy Advanced Certificate Manager. The name is one setting, `COLOPHON_PUBLIC_URL` in `/etc/colophon/colophon.env`; another name later means changing that line and running `install.sh` again |
| 8 | Cloudflare SSL/TLS | Mode **Full (strict)**; an **Origin Certificate** covering `colophonapp.404human.dev` (RSA, 15 years; the default hostnames `*.404human.dev` and `404human.dev` cover it); and **Authenticated Origin Pulls** on, zone-level (SSL/TLS → Origin Server). Cloudflare's addresses are shared by all its customers, so the origin also requires a client certificate. Cloudflare's default one is **shared by every Cloudflare zone** too: the end state is our own, uploaded to this zone (DEPLOY.md, "Our own origin-pull certificate"; an API token with Zone → SSL and Certificates → Edit, for this zone only, used once) | The certificate and its private key (pasted on the droplet only) |
| 9 | Cloudflare | Web Analytics **off** for this hostname (it injects a script the CSP blocks) | — |
| 10 | Email (optional for S0) | A Resend API key and a verified sender domain, only if you want sign-in links by email | The key |

What you'll paste on the droplet, and where (DEPLOY.md walks you through each):

- the Neon owner connection string → `/etc/colophon/credentials/migration_database_url`;
- the media token's secret access key → `/etc/colophon/credentials/s3_secret_key`;
- the backups token's secret access key → `/etc/colophon/credentials/backup_s3_secret_key`;
- the origin certificate and key → `/etc/caddy/origin.pem` and `/etc/caddy/origin.key`;
- the R2 account id and both access key ids (not secret) → `/etc/colophon/colophon.env`.

The app's own database password and the cache password are generated on the droplet; you never
see them. One thing generated there you **must** copy out: the backup age key's private line
(DEPLOY.md step 10), into a password manager or onto paper. The logical backups are unreadable
without it.
