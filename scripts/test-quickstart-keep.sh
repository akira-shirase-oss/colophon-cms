#!/usr/bin/env bash
# scripts/quickstart.sh's exit: without --keep it removes the project and its folder; with --keep it leaves both,
# so `docker compose` in that folder still finds the project. (The folder used to be deleted even with --keep,
# which left a kept project's logs and setup code reachable only through `docker logs <container>`.)
# Docker is a stub here: the test records what would be run, so it needs no daemon.
set -euo pipefail
cd "$(dirname "$0")/.."

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/bin"
cat >"$tmp/bin/docker" <<'SH'
#!/usr/bin/env bash
echo "$*" >>"$DOCKER_CALLS"
SH
chmod +x "$tmp/bin/docker"
export PATH="$tmp/bin:$PATH" DOCKER_CALLS="$tmp/calls"

fail() { echo "test-quickstart-keep: $*" >&2; exit 1; }
source scripts/quickstart-cleanup.sh
export COMPOSE_PROJECT_NAME=colophon-quickstart-check-0000
origin=http://localhost:0000
compose() { (cd "$work" && docker compose "$@"); }

# --keep: nothing stopped, the folder and its compose file stay, and the output says where.
keep=true work=$(mktemp -d "$tmp/kept.XXXXXX"); : >"$work/compose.yaml"; : >"$DOCKER_CALLS"
out=$(quickstart_cleanup 0)
[[ -f $work/compose.yaml ]] || fail "--keep deleted the project's folder"
grep -q "down" "$DOCKER_CALLS" && fail "--keep stopped the project: $(cat "$DOCKER_CALLS")"
[[ $out == *"$work"* && $out == *"down --volumes"* ]] || fail "--keep didn't say where the project is and how to remove it: $out"

# Without --keep: the project goes (down --volumes), then its folder.
keep=false work=$(mktemp -d "$tmp/gone.XXXXXX"); : >"$work/compose.yaml"; : >"$DOCKER_CALLS"
quickstart_cleanup 0 >/dev/null
[[ -d $work ]] && fail "without --keep the project's folder was left behind"
grep -q "compose down --volumes" "$DOCKER_CALLS" || fail "without --keep the project wasn't removed: $(cat "$DOCKER_CALLS")"

# A failed run prints the services' logs first, with or without --keep.
keep=true work=$(mktemp -d "$tmp/failed.XXXXXX"); : >"$DOCKER_CALLS"
quickstart_cleanup 1 >/dev/null
grep -q "compose logs" "$DOCKER_CALLS" || fail "a failed run didn't print the services' logs"

echo "test-quickstart-keep: --keep keeps the project and its folder; without it both go; a failure shows the logs"
