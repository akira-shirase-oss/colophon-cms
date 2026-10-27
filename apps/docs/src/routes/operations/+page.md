<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon is pre-release, so "operations" today means running it on your own machine and reproducing the evidence behind it. Running it in production, with backups, restores and upgrades, is milestone P6.

- [The local stack](/operations/local-stack/): PostgreSQL, Valkey, Garage and Keycloak in Docker, bound to `127.0.0.1`, and the checks that prove each one works. <Status implemented />
- [Configuration](/operations/configuration/): every `COLOPHON_*` setting the server reads, its default and its limits. <Status implemented />
- [Load testing](/operations/load-testing/): `scripts/load.sh`, and every result kept in `perf/results/`, failures included, each with the machine it ran on. <Status implemented />
- [Crash tests and the crash loop](/operations/crash-tests/): kill the server and the worker on purpose, and check nothing was lost or doubled. <Status implemented />
- [Backups, restore and upgrades](/operations/backup-and-upgrades/): what the runbooks will cover. <Status planned="P6" />

## The one command CI runs

`scripts/check.sh` runs exactly what CI runs: the toolchain check, formatting, Clippy with warnings as errors, the tests, the docs build, the architecture rule, `cargo deny`, ShellCheck, both simulator sweeps, and, when the local stack is up, the integration half (the tests that stop and restart services, the crash tests, the testbed and the Studio in Chromium, and short load runs). Contributors run it before every push.

It takes a while and needs the whole stack. These docs never ask you to run it to try Colophon; the [quickstart](/get-started/quickstart/) is enough for that.
