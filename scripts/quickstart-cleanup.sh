# shellcheck shell=bash
# shellcheck disable=SC2154 # keep, work and origin are the caller's (see below).
# Sourced by scripts/quickstart.sh, and by scripts/test-quickstart-keep.sh: what happens to the quick-start
# project when the script exits. It uses the caller's `compose`, `work`, `keep`, `origin` and COMPOSE_PROJECT_NAME.
#
# Without --keep: the project's containers and volumes go, then its folder.
# With --keep: nothing is stopped or deleted, and the folder holding the project's compose.yaml stays, so
# `docker compose` there still finds the project (its logs, its setup code, `down` when you're done).

quickstart_cleanup() {
  local rc=$1
  if [[ $rc != 0 ]]; then
    echo "--- the services' logs, for the failure above"
    compose logs --no-color --tail 60 2>/dev/null || true
  fi
  if [[ $keep == true ]]; then
    echo "kept: project $COMPOSE_PROJECT_NAME at $origin (use localhost: sign-in trusts that address)"
    echo "  its files: $work"
    echo "  logs:      (cd \"$work\" && docker compose logs colophon)"
    echo "  remove it: (cd \"$work\" && docker compose down --volumes) && rm -rf \"$work\""
    return 0
  fi
  compose down --volumes --timeout 10 >/dev/null 2>&1 || true
  rm -rf "$work"
}
