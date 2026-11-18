# shellcheck shell=bash
# Whose stack is this? (D-187) Sourced by check.sh, load.sh and stack.sh before anything that
# stops, pauses or kills a stack service, or changes the database cluster's roles.
#
# deploy/compose.yaml names its project `colophon`, so from any checkout or worktree on a machine,
# `docker compose -f deploy/compose.yaml` reaches the same everyday stack. A gate run from a
# worktree once restarted another person's Postgres that way. The stack counts as ours only when:
#
#   - CI is set (the job made its own stack), or
#   - COMPOSE_PROJECT_NAME names a project other than `colophon` (a stack of our own), or
#   - COLOPHON_STACK_OWNER=1 (whoever owns the everyday stack says so, for their own runs).
#
# Otherwise the stack is shared: reading it, and databases of our own in it, are fine; stopping,
# pausing or restarting its services and running role DDL are not.

# 0 when the stack this checkout's compose file reaches is ours to stop or reconfigure.
stack_is_ours() {
  [[ -n "${CI:-}" ]] && return 0
  [[ -n "${COMPOSE_PROJECT_NAME:-}" && "${COMPOSE_PROJECT_NAME}" != colophon ]] && return 0
  [[ "${COLOPHON_STACK_OWNER:-}" == 1 ]] && return 0
  return 1
}

# Exits 2, naming WHAT was refused and how to proceed, unless the stack is ours.
require_own_stack() {
  stack_is_ours && return 0
  cat >&2 <<EOF
error: refusing to $1 on the shared 'colophon' stack (D-187): it isn't this run's to stop, pause or
reconfigure. Use a stack of your own (COMPOSE_PROJECT_NAME=<name>, with its own ports), let CI run
it, or, if the everyday stack is yours, set COLOPHON_STACK_OWNER=1.
EOF
  exit 2
}
