# shellcheck shell=bash
# Sourced by the scripts that start servers on fixed ports (load.sh, testbed.sh, studio.sh).
#
# A port that already has a listener means another process would answer in place of the one about
# to start: the child dies on EADDRINUSE while the squatter, with its own state, answers the
# readiness probe and gets measured or tested. (A stale receiver once drove a real Valkey pause
# through its /control while the run's own receiver.log held only the crash dump, so the warn
# window read empty and every warning counted.) Fail fast with a way to find it.
port_busy() { lsof -nP -t -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }
refuse_squatter() {
  if port_busy "$1"; then
    echo "error: port $1 already has a listener — another process would answer in place" >&2
    echo "of this run's own $2. Find it: lsof -nP -iTCP:$1 -sTCP:LISTEN" >&2
    exit 1
  fi
}
