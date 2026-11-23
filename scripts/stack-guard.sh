# shellcheck shell=bash
# Whose stack is this? (D-187) Sourced by check.sh, load.sh and stack.sh before anything that
# stops, pauses or kills a stack service, or changes the database cluster's roles.
#
# deploy/compose.yaml names its project `colophon`, so from any checkout or worktree on a machine,
# `docker compose -f deploy/compose.yaml` reaches the same everyday stack. A gate run from a
# worktree once restarted another person's Postgres that way. The stack counts as ours only when:
#
#   - CI=true exactly, on a GitHub runner: GITHUB_ACTIONS=true and RUNNER_NAME set (the job made
#     its own stack; CI=false, 0 or 1 doesn't count, D-187b; CI=true alone, which local tooling
#     and agents export, doesn't either, D-187c), or
#   - COMPOSE_PROJECT_NAME names a project other than `colophon` (a stack of our own, its own ports).
#
# Otherwise the stack is shared: reading it, and databases of our own in it, are fine; stopping,
# pausing, restarting or flushing its services and running role DDL are not. A person at a
# terminal may still do it to the stack on purpose (`stack.sh down`, `migrate.sh` on `colophon`):
# operator_confirms asks them to type the project's name; without a terminal it never does (D-187c).

# 0 when the stack this checkout's compose file reaches is ours to stop or reconfigure.
stack_is_ours() {
  [[ "${CI:-}" == true && "${GITHUB_ACTIONS:-}" == true && -n "${RUNNER_NAME:-}" ]] && return 0
  [[ -n "${COMPOSE_PROJECT_NAME:-}" && "${COMPOSE_PROJECT_NAME}" != colophon ]] && return 0
  return 1
}

# 0 when a person at a terminal confirms WHAT on the shared stack by typing its project's name.
# Never without a terminal on both stdin and stderr: a script, a test or an agent can't answer.
operator_confirms() {
  [[ -t 0 && -t 2 ]] || return 1
  local answer=""
  printf '%s on the shared colophon stack. Type colophon to go ahead: ' "$1" >&2
  IFS= read -r answer || return 1
  [[ "$answer" == colophon ]]
}

# Exits 2, naming WHAT was refused and how to proceed, unless the stack is ours.
require_own_stack() {
  stack_is_ours && return 0
  cat >&2 <<EOF
error: refusing to $1 on the shared 'colophon' stack (D-187): it isn't this run's to stop, pause or
reconfigure. Use a stack of your own (COMPOSE_PROJECT_NAME=<name>, with its own ports), or let CI run
it. If you mean to do it to the everyday stack yourself, run the command at a terminal and confirm.
EOF
  exit 2
}
