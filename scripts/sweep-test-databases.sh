#!/usr/bin/env bash
# Drops the databases earlier test runs left on the stack: `colophon_test_*` and
# `colophon_setup_*` (tests make one each and drop it as they end; a run killed outright still
# leaves them), but only those with no connection at all, so a run going on beside this one keeps
# its own. Never touches any other database: the names are matched exactly, `colophon` among the
# untouched. Each drop is plain DROP DATABASE, without FORCE, so a database that gains a connection
# between the look and the drop is skipped, not cut off.
#
# It changes the stack, so it runs only on a stack that is this run's own (D-187: CI, a
# COMPOSE_PROJECT_NAME of its own, or COLOPHON_STACK_OWNER=1). check.sh and CI run it before the
# stack tests.
#
#   scripts/sweep-test-databases.sh
set -euo pipefail
cd "$(dirname "$0")/.."
# shellcheck source=scripts/stack-guard.sh
. scripts/stack-guard.sh
require_own_stack "drop leftover test databases"

psql_admin() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -XAtq -v ON_ERROR_STOP=1 -c "$1"
}

leftovers=$(psql_admin "SELECT d.datname FROM pg_database d
  WHERE (d.datname ~ '^colophon_test_[0-9a-f]{32}\$' OR d.datname ~ '^colophon_setup_[0-9a-f]{32}\$')
    AND NOT EXISTS (SELECT 1 FROM pg_stat_activity a WHERE a.datname = d.datname)
  ORDER BY d.datname")

dropped=0
skipped=0
# The names come in on fd 3: `docker compose exec -T` reads standard input.
while IFS= read -r name <&3; do
  [[ -z "$name" ]] && continue
  # The pattern above already pins the shape; checked again before the name goes in SQL.
  [[ "$name" =~ ^colophon_(test|setup)_[0-9a-f]{32}$ ]] || continue
  if psql_admin "DROP DATABASE IF EXISTS $name" >/dev/null 2>&1; then
    dropped=$((dropped + 1))
  else
    skipped=$((skipped + 1))
  fi
done 3<<<"$leftovers"
note=""
((skipped > 0)) && note="; $skipped gained a connection and were left"
echo "sweep: dropped $dropped leftover test database(s)$note"
