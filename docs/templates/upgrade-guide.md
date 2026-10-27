<!-- Upgrade guide template (docs/RELEASES.md): one per minor release, as docs/upgrading/0.N.md. -->
# Upgrading to Colophon 0.N

Applies to 0.(N-1).x → 0.N.x. Time on the reference profile: ~X minutes. Downtime: <none / Y>.

## Before you start
- [ ] Read the 0.N release notes, especially "Breaking changes".
- [ ] Note your versions: `colophon-server --version`, `colophon --version`.
- [ ] On 0.(N-2) or older? Upgrade to 0.(N-1) first. Versions can't be skipped before 1.0.
- [ ] Take a backup and restore it somewhere once. A backup you haven't restored is a hope.
- [ ] If you can, rehearse on a copy first.

## Are you affected?
For each breaking change, a check that answers yes or no, e.g.:
`colophon admin check <thing>` or the SQL query to run. (Directus tells people to "review the
`ip_access` column" by hand [D2]; Colophon can ship the query.)

## What changes for you
- **Editors:** <plain words; screenshots>
- **Developers** (API, SDK, your site's build): <before/after code for each change>
- **Operators** (settings, environment variables, PostgreSQL/Valkey versions): <list>

## Steps
1. Stop the worker (`--role worker`), so no job runs across the upgrade.
2. Apply migrations 00xx to 00yy: <command>. They are <additive / include a removal step>.
3. Replace the binaries; start the server, then the worker.
4. Wait for `/readyz` to report ready.

## Check it worked
- `colophon whoami` answers with your identity.
- Publish a test entry in a scratch project and watch its receipt reach
  *Verified at N locations*.
- Your site still builds from `/content/v1` (run its build once).

## Rolling back
- Before migration 00yy ran: redeploy the 0.(N-1) binaries.
- After it ran: restore the backup (you lose writes made since it), following the operations guide.

## Known issues
