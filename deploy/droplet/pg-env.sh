# shellcheck shell=bash
# Sourced by smoke.sh (and its test): a database URL holds the password, and a command line is
# readable by every local user (ps, /proc/<pid>/cmdline), so psql is never given the URL. Its parts
# go into libpq's environment instead, which only root and the process's own user can read, as
# backup-logical.sh does for pg_dump.
#
# pg_url_run <url> <command> [args...]: runs the command with PGHOST, PGPORT (5432 if absent),
# PGUSER, PGPASSWORD (percent-decoded), PGDATABASE, and PGSSLMODE / PGSSLROOTCERT when the URL's
# query names them (PGSSLROOTCERT=system for a verify-* URL that names no CA). All of it only in the
# command's own environment, never the caller's. A URL it can't read runs nothing and returns 2.
pg_url_run() {
  local url=$1 user pass host port db query sslmode="" rootcert=""
  shift
  [[ "$url" =~ ^postgres(ql)?://([^:@/]+):([^@/]+)@([^:/?]+)(:([0-9]+))?/([^?]+)(\?(.*))?$ ]] \
    || { echo "error: can't read the database URL's parts" >&2; return 2; }
  user=${BASH_REMATCH[2]} pass=${BASH_REMATCH[3]} host=${BASH_REMATCH[4]} port=${BASH_REMATCH[6]:-5432}
  db=${BASH_REMATCH[7]} query=${BASH_REMATCH[9]:-}
  pass=$(printf '%b' "${pass//%/\\x}")
  if [[ "&$query&" =~ \&sslmode=([^\&]+)\& ]]; then sslmode=${BASH_REMATCH[1]}; fi
  if [[ "&$query&" =~ \&sslrootcert=([^\&]+)\& ]]; then rootcert=$(printf '%b' "${BASH_REMATCH[1]//%/\\x}"); fi
  # A URL that verifies without naming its CA (Neon's, as install.sh writes it) means the public
  # roots, as the server reads it: libpq's word for them is "system" (without it, libpq looks for
  # ~/.postgresql/root.crt and fails). Set for psql alone, below: sqlx reads PGSSLROOTCERT as a file
  # path, so the colophon CLI must never inherit it.
  if [[ -z "$rootcert" && ( "$sslmode" == verify-full || "$sslmode" == verify-ca ) ]]; then rootcert=system; fi
  (
    export PGHOST=$host PGPORT=$port PGUSER=$user PGPASSWORD=$pass PGDATABASE=$db
    [[ -n "$sslmode" ]] && export PGSSLMODE=$sslmode
    [[ -n "$rootcert" ]] && export PGSSLROOTCERT=$rootcert
    exec "$@"
  )
}
