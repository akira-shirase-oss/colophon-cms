# shellcheck shell=bash
# Sourced (`. scripts/local-env.sh`) by scripts that need the local stack's settings: exports
# deploy/local.env. With COLOPHON_TEST_DATABASE set to a database name, both database URLs name it
# instead of `colophon`, so a checkout whose migrations are ahead of the others (a step branch)
# is tested against its own database and never migrates the one they share. The Rust stack tests
# and the Playwright configs read the same variable.
set -a
# shellcheck source=/dev/null # plain KEY=value lines
. deploy/local.env
set +a
if [[ -n "${COLOPHON_TEST_DATABASE:-}" ]]; then
  if [[ ! "$COLOPHON_TEST_DATABASE" =~ ^[a-z][a-z0-9_]{0,62}$ ]]; then
    echo "error: COLOPHON_TEST_DATABASE must be a plain database name" >&2
    exit 2
  fi
  export COLOPHON_DATABASE_URL="${COLOPHON_DATABASE_URL%/colophon}/$COLOPHON_TEST_DATABASE"
  export COLOPHON_MIGRATION_DATABASE_URL="${COLOPHON_MIGRATION_DATABASE_URL%/colophon}/$COLOPHON_TEST_DATABASE"
fi
