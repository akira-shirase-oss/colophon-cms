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
# A stack of its own on other ports (a COMPOSE_PROJECT_NAME other than colophon, D-187c):
# deploy/compose.yaml publishes Postgres, Valkey and Garage's S3 on COLOPHON_PG_PORT,
# COLOPHON_VALKEY_PORT and COLOPHON_S3_PORT when they're set, and the URLs follow them here.
# The loop's names are prefixed and unset after it: this file is sourced into scripts with their own.
for _colophon_published in PG:55432 VALKEY:56379 S3:53900; do
  _colophon_port_var="COLOPHON_${_colophon_published%%:*}_PORT"
  _colophon_port="${!_colophon_port_var:-}"
  [[ -n "$_colophon_port" ]] || continue
  if [[ ! "$_colophon_port" =~ ^[1-9][0-9]{0,4}$ ]]; then
    echo "error: $_colophon_port_var must be a port number" >&2
    exit 2
  fi
  for _colophon_url_var in COLOPHON_DATABASE_URL COLOPHON_MIGRATION_DATABASE_URL COLOPHON_CACHE_URL COLOPHON_S3_ENDPOINT; do
    export "$_colophon_url_var=${!_colophon_url_var//:${_colophon_published##*:}/:$_colophon_port}"
  done
done
unset _colophon_published _colophon_port_var _colophon_port _colophon_url_var
if [[ -n "${COLOPHON_TEST_DATABASE:-}" ]]; then
  if [[ ! "$COLOPHON_TEST_DATABASE" =~ ^[a-z][a-z0-9_]{0,62}$ ]]; then
    echo "error: COLOPHON_TEST_DATABASE must be a plain database name" >&2
    exit 2
  fi
  export COLOPHON_DATABASE_URL="${COLOPHON_DATABASE_URL%/colophon}/$COLOPHON_TEST_DATABASE"
  export COLOPHON_MIGRATION_DATABASE_URL="${COLOPHON_MIGRATION_DATABASE_URL%/colophon}/$COLOPHON_TEST_DATABASE"
fi
