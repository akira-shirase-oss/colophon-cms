#!/usr/bin/env bash
# Tests pg-env.sh's pg_url_run against a stand-in psql that records its command line and the
# libpq environment it was given:
#   - the password reaches psql only as PGPASSWORD (percent-decoded), never on its command line;
#   - host, port (5432 when absent), database, sslmode and sslrootcert come from the URL;
#   - a URL it can't read runs nothing;
#   - smoke.sh never hands psql a URL on its command line (psql "$(cat ...)").
#
#   deploy/droplet/test-pg-env.sh
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
mkdir -p "$work/bin"
cat >"$work/bin/psql" <<'STUB'
#!/usr/bin/env bash
{ printf 'argv=%s\n' "$*"; env | grep -E '^PG' | sort; } >"$OUT"
STUB
chmod +x "$work/bin/psql"
# shellcheck source=deploy/droplet/pg-env.sh
. ./pg-env.sh

OUT=$work/out PATH="$work/bin:$PATH" pg_url_run 'postgres://colophon_app:s3cr%2Bet%40x@ep-x.us-east-2.aws.neon.tech/colophon?sslmode=verify-full&sslrootcert=system' \
  psql -XAtqc 'select 1'
grep -qx 'argv=-XAtqc select 1' "$work/out" || fail "psql's command line: $(grep argv "$work/out")"
if grep '^argv=' "$work/out" | grep -q 's3cr'; then fail "the password is on psql's command line"; fi
for want in 'PGPASSWORD=s3cr+et@x' 'PGUSER=colophon_app' 'PGHOST=ep-x.us-east-2.aws.neon.tech' 'PGPORT=5432' \
  'PGDATABASE=colophon' 'PGSSLMODE=verify-full' 'PGSSLROOTCERT=system'; do
  grep -qx "$want" "$work/out" || fail "missing $want: $(grep -v PGPASSWORD "$work/out")"
done
echo "  ok    the password reaches psql as PGPASSWORD (decoded), never argv; host, port, database and TLS from the URL"

# Neon's URL, as install.sh writes it: verify-full and no sslrootcert. libpq would look for
# ~/.postgresql/root.crt and fail; psql gets sslrootcert=system, and only psql: the caller's shell
# (where the colophon CLI runs next; sqlx reads PGSSLROOTCERT as a file) never has it.
OUT=$work/out4 PATH="$work/bin:$PATH" pg_url_run 'postgres://colophon_app:pw@ep-x.us-east-2.aws.neon.tech/colophon?sslmode=verify-full' psql -c x
grep -qx 'PGSSLROOTCERT=system' "$work/out4" || fail "verify-full without sslrootcert: psql wasn't given system roots: $(grep -v PGPASSWORD "$work/out4")"
# shellcheck disable=SC2031  # that the subshell's export is lost here is what this checks
[[ -z "${PGSSLROOTCERT:-}" ]] || fail "PGSSLROOTCERT leaked into the caller's shell"
if env | grep -qE '^PG(PASSWORD|SSLROOTCERT)='; then fail "PGPASSWORD or PGSSLROOTCERT leaked into the caller's environment"; fi
echo "  ok    verify-full without sslrootcert: psql alone gets PGSSLROOTCERT=system; nothing leaks to the caller (the CLI)"

OUT=$work/out2 PATH="$work/bin:$PATH" pg_url_run 'postgresql://owner:pw@db.example.com:6543/app?sslmode=require' psql -c x
if ! grep -qx 'PGPORT=6543' "$work/out2" || ! grep -qx 'PGSSLMODE=require' "$work/out2"; then fail "an explicit port and sslmode: $(cat "$work/out2")"; fi
if grep -q '^PGSSLROOTCERT=' "$work/out2"; then fail "sslmode=require got a root certificate setting it didn't ask for"; fi

echo "  ok    an explicit port and sslmode are kept"

rm -f "$work/out3"
if OUT=$work/out3 PATH="$work/bin:$PATH" pg_url_run 'not a url' psql -c x 2>/dev/null; then fail "a URL it can't read was run"; fi
[[ ! -e "$work/out3" ]] || fail "psql ran for a URL it couldn't read"
echo "  ok    a URL it can't read runs nothing"

if grep -nE 'psql[^#]*"\$\(cat' smoke.sh; then fail "smoke.sh puts a database URL on psql's command line"; fi
if grep -nE 'authorization: Bearer \$' smoke.sh | grep -v '>"' ; then fail "smoke.sh puts the token on curl's command line"; fi
echo "  ok    smoke.sh passes no database URL or token on a command line"
echo "pg-env: database passwords reach psql through libpq's environment only"
