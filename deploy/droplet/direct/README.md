# Direct deploy (D-242)

Deploys a commit straight to the droplet, with no CI or release build in the way: about 2–5 minutes.

```sh
COLOPHON_DEPLOY_HOST=<ssh alias of the droplet> COLOPHON_PUBLIC_URL=https://<your host> \
  deploy/droplet/direct/deploy.sh origin/main
```

1. The droplet builds the server and CLI for the commit (`droplet-build.sh`, incremental, cached in
   `/opt/colophon-build`, low CPU priority), detached so a dropped SSH connection can't stall it.
2. The Studio is built on this machine at the same commit and uploaded.
3. `droplet-deploy.sh` installs it with `upgrade.sh`'s safety steps: refuse a database migrated by a
   newer build, migrate while the old release serves, switch atomically, `/readyz` + `smoke.sh`, and
   roll back automatically if any check fails. It keeps the current and previous releases only,
   deletes the upload, and wipes the build cache past 10 GB.
4. `/version`, `/readyz`, `/studio/` and `/studio/signup` are checked through the public URL.

The first build on a new droplet installs the pinned toolchain and takes about 11 minutes; later
builds take a few. Tagged multi-platform releases (`release.yml`) are for versions, not for getting
a fix live.
