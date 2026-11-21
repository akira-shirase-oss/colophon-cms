#!/usr/bin/env bash
# Colophon installed by a role that is not a superuser (D-234), the way Neon installs it: Neon's
# database owner has CREATEROLE and CREATEDB, no superuser, and no SET on any parameter.
#
# A throwaway postgres container (this script's own, on 127.0.0.1, never the stack's, removed on
# exit) with such an owner, and the real `colophon` binary as that owner:
#
#   admin init-roles, admin migrate, admin migrate-status       succeed; the schema is current
#   the run's CREATE grants on public                           revoked afterwards
#   _sqlx_migrations for 0011, 0012, 0051, 0082                 the original files' checksums
#   delivery_environment / media_object                         no app.* setting pinned on them
#   after a call, the caller's app.project_id and audience      as they were (set, and unset)
#   after an error inside a call, caught by a savepoint         as they were
#
# COLOPHON_BIN picks the binary (default target/release/colophon, built if missing);
# COLOPHON_NONSUPER_CHECK_PORT the port (default 55451, bumped while taken). Needs docker and jq.
# Runs on macOS's bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${COLOPHON_NONSUPER_CHECK_PORT:-55451}"
COLOPHON_BIN="${COLOPHON_BIN:-target/release/colophon}"
CONTAINER="colophon-nonsuper-check-$$"
# The container's own passwords; they never leave this script.
SUPER_PASSWORD="nonsuper-check-super-$$"
OWNER_PASSWORD="nonsuper-check-owner-$$"
APP_PASSWORD="nonsuper-check-app-$$-$RANDOM$RANDOM"

checks=0
failures=0
pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }
# expect <what> <wanted> <got>
expect() { if [[ "$3" == "$2" ]]; then pass "$1"; else fail "$1: wanted '$2', got '$3'"; fi; }

work=$(mktemp -d)
cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$work"
}
trap cleanup EXIT

if [[ ! -x "$COLOPHON_BIN" ]]; then
  cargo build --quiet --release --locked -p colophon-cli
fi
while (echo >"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; do PORT=$((PORT + 1)); done
image=$(docker compose -f deploy/compose.yaml config --format json | jq -r '.services.postgres.image')

docker run -d --name "$CONTAINER" -p "127.0.0.1:$PORT:5432" \
  -e POSTGRES_USER=postgres -e "POSTGRES_PASSWORD=$SUPER_PASSWORD" "$image" >/dev/null
ready=0
for _ in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -q -h 127.0.0.1 -p 5432 >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  docker logs "$CONTAINER" 2>&1 | tail -20 >&2
  echo "error: the postgres container never became ready (logs above)" >&2
  exit 1
fi

# super <sql>: as the container's superuser, on the colophon database once it exists.
super() { docker exec -i "$CONTAINER" psql -U postgres -d "${SUPER_DB:-colophon}" -XAtq -v ON_ERROR_STOP=1 "$@"; }
# app <<SQL: as colophon_app over TCP, errors reported but not fatal (the savepoint checks).
app() {
  docker exec -i -e "PGPASSWORD=$APP_PASSWORD" "$CONTAINER" \
    psql -h 127.0.0.1 -U colophon_app -d colophon -XAtq 2>&1
}

# Neon's shape: an owner with CREATEROLE and CREATEDB, not a superuser, owning the database.
SUPER_DB=postgres super -c "CREATE ROLE neon_owner LOGIN CREATEROLE CREATEDB NOSUPERUSER PASSWORD '$OWNER_PASSWORD'"
SUPER_DB=postgres super -c "CREATE DATABASE colophon OWNER neon_owner"
printf '%s' "$APP_PASSWORD" >"$work/app_password"
url="postgres://neon_owner:$OWNER_PASSWORD@127.0.0.1:$PORT/colophon"
echo "Colophon as a non-superuser owner (postgres on 127.0.0.1:$PORT, $("$COLOPHON_BIN" --version)):"
expect "the owner is not a superuser and holds SET on no parameter" "f|0" \
  "$(super -c "SELECT CASE WHEN rolsuper THEN 't' ELSE 'f' END || '|' || (SELECT count(*) FROM pg_parameter_acl) FROM pg_roles WHERE rolname = 'neon_owner'")"

# cli <args>: the URL in the environment (never an argument); output in $out, exit code in $code.
cli() {
  code=0
  out=$(COLOPHON_MIGRATION_DATABASE_URL="$url" "$COLOPHON_BIN" "$@" 2>&1) || code=$?
}
cli admin init-roles --app-password-file "$work/app_password"
if [[ "$code" == 0 ]]; then pass "admin init-roles"; else fail "admin init-roles (exit $code): $(tail -3 <<<"$out")"; fi
cli admin migrate
if [[ "$code" == 0 ]]; then pass "admin migrate"; else fail "admin migrate (exit $code): $(tail -3 <<<"$out")"; fi
cli admin migrate-status
if [[ "$code" == 0 && "$out" == *"schema current"* ]]; then pass "admin migrate-status: $out"; else fail "admin migrate-status (exit $code): $(tail -3 <<<"$out")"; fi
if [[ "$failures" -gt 0 ]]; then
  echo "non-superuser install: $failures of $checks checks failed" >&2
  exit 1
fi

# grants_after <run>: what the run must leave behind, checked after the first install and again
# after init-roles and migrate run a second time (a re-run install, an upgrade).
grants_after() {
  expect "$1: no colophon role keeps CREATE on public" "none" "$(super -c "
    SELECT coalesce(string_agg(r.rolname, ',' ORDER BY r.rolname), 'none')
    FROM pg_namespace n, aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) a
    JOIN pg_roles r ON r.oid = a.grantee
    WHERE n.nspname = 'public' AND a.privilege_type = 'CREATE' AND r.rolname LIKE 'colophon\_%'")"
  expect "$1: the owner can SET ROLE to every colophon role (missing on: none)" "none" "$(super -c "
    SELECT coalesce(string_agg(r.rolname, ',' ORDER BY r.rolname), 'none') FROM pg_roles r
    WHERE r.rolname LIKE 'colophon\_%' AND NOT EXISTS (
      SELECT FROM pg_auth_members m WHERE m.roleid = r.oid AND m.set_option
        AND m.member = (SELECT oid FROM pg_roles WHERE rolname = 'neon_owner'))")"
  # Migration 0048 makes colophon_deleter's INHERIT FALSE on purpose (the ledger replay runs by
  # SET ROLE): no run may turn it back on.
  expect "$1: the owner inherits every colophon role but colophon_deleter" "colophon_deleter" "$(super -c "
    SELECT coalesce(string_agg(r.rolname, ',' ORDER BY r.rolname), 'none') FROM pg_roles r
    WHERE r.rolname LIKE 'colophon\_%' AND NOT EXISTS (
      SELECT FROM pg_auth_members m WHERE m.roleid = r.oid AND m.inherit_option
        AND m.member = (SELECT oid FROM pg_roles WHERE rolname = 'neon_owner'))")"
}
grants_after "after the install"
cli admin init-roles --app-password-file "$work/app_password"
if [[ "$code" == 0 ]]; then pass "admin init-roles again"; else fail "admin init-roles again (exit $code): $(tail -3 <<<"$out")"; fi
cli admin migrate
if [[ "$code" == 0 ]]; then pass "admin migrate again"; else fail "admin migrate again (exit $code): $(tail -3 <<<"$out")"; fi
grants_after "after a second run"
# D-235: 0087 lifts FORCE on projects and memberships for its backfill; it must be back on.
expect "row security forced again on memberships and projects" "memberships|t,projects|t" "$(super -c "
  SELECT string_agg(relname || '|' || CASE WHEN relforcerowsecurity THEN 't' ELSE 'f' END, ',' ORDER BY relname)
  FROM pg_class WHERE relname IN ('projects', 'memberships') AND relkind = 'r'")"
expect "no app.* setting is pinned on the two functions" "0" "$(super -c "
  SELECT count(*) FROM pg_proc, unnest(proconfig) AS setting
  WHERE proname IN ('delivery_environment', 'media_object') AND setting LIKE 'app.%'")"
for version in 11 12 51 82; do
  file=$(ls crates/colophon-db/migrations/"$(printf '%04d' "$version")"_*.sql)
  wanted=$(shasum -a 384 "$file" | cut -d' ' -f1)
  expect "migration $version is recorded with the original file's checksum" "$wanted" \
    "$(super -c "SELECT encode(checksum, 'hex') FROM _sqlx_migrations WHERE version = $version")"
done

# One project with a production environment, written past row security as the superuser.
project=11111111-1111-4111-8111-111111111111
other=22222222-2222-4222-8222-222222222222
super -c "INSERT INTO projects (id, slug, name) VALUES ('$project', 'nonsuper-check', 'nonsuper check');
          INSERT INTO environments (project_id, id, slug) VALUES ('$project', gen_random_uuid(), 'production')" >/dev/null

# After a call: the caller's values, set (another project and an audience) and unset.
got=$(app <<SQL
BEGIN;
SELECT set_config('app.project_id', '$other', true), set_config('app.audience_role', 'members', true);
SELECT 'env:' || count(*) FROM delivery_environment('$project', 'production');
SELECT 'after-env:' || current_setting('app.project_id') || '|' || current_setting('app.audience_role');
SELECT 'media:' || count(*) FROM media_object('$project', 'production', gen_random_uuid());
SELECT 'after-media:' || current_setting('app.project_id') || '|' || current_setting('app.audience_role');
COMMIT;
SQL
)
expect "delivery_environment reads its own project, whatever the caller's" "env:1" "$(grep '^env:' <<<"$got")"
expect "the caller's project and audience after delivery_environment" "after-env:$other|members" "$(grep '^after-env:' <<<"$got")"
expect "media_object reads its own project, whatever the caller's" "media:1" "$(grep '^media:' <<<"$got")"
expect "the caller's project and audience after media_object" "after-media:$other|members" "$(grep '^after-media:' <<<"$got")"
got=$(app <<SQL
SELECT 'env:' || count(*) FROM delivery_environment('$project', 'production');
SELECT 'after-env:' || coalesce(nullif(current_setting('app.project_id', true), ''), 'unset');
SELECT 'media:' || count(*) FROM media_object('$project', 'production', gen_random_uuid());
SELECT 'after-media:' || coalesce(nullif(current_setting('app.project_id', true), ''), 'unset')
       || '|' || coalesce(nullif(current_setting('app.audience_role', true), ''), 'unset');
SQL
)
expect "no project context outlives delivery_environment for a caller with none" "after-env:unset" "$(grep '^after-env:' <<<"$got")"
expect "no project or audience context outlives media_object for a caller with none" "after-media:unset|unset" "$(grep '^after-media:' <<<"$got")"

# After an error inside the call (a lock timeout on environments, after the function has set its
# project), caught by a savepoint: the caller's values.
for call in "delivery_environment('$project', 'production')" "media_object('$project', 'production', gen_random_uuid())"; do
  name=${call%%(*}
  # Holds environments for a few seconds from another session.
  super -c "BEGIN; LOCK TABLE environments IN ACCESS EXCLUSIVE MODE; SELECT pg_sleep(4); COMMIT;" >/dev/null &
  holder=$!
  for _ in $(seq 1 50); do
    [[ "$(super -c "SELECT count(*) FROM pg_locks l JOIN pg_class c ON c.oid = l.relation WHERE c.relname = 'environments' AND l.mode = 'AccessExclusiveLock' AND l.granted")" == 1 ]] && break
    sleep 0.1
  done
  got=$(app <<SQL
BEGIN;
SELECT set_config('app.project_id', '$other', true), set_config('app.audience_role', 'members', true);
SET LOCAL lock_timeout = '300ms';
SAVEPOINT before_call;
SELECT count(*) FROM $call;
ROLLBACK TO SAVEPOINT before_call;
SELECT 'after:' || current_setting('app.project_id') || '|' || current_setting('app.audience_role');
COMMIT;
SQL
)
  wait "$holder" || true
  if grep -q 'lock timeout' <<<"$got"; then pass "$name failed inside the call (lock timeout)"; else fail "$name didn't fail inside the call: $got"; fi
  expect "the caller's project and audience after $name's error, caught by a savepoint" "after:$other|members" "$(grep '^after:' <<<"$got")"
done

# A run held open inside a migration, then killed (D-234): each migration's grants are its own
# transaction's. A second database in the same cluster (roles are cluster-wide, as on Neon):
# _sqlx_migrations made first with sqlx's own DDL, and another session holding an uncommitted row
# for version 2, so the run commits migration 1 (prologue, body, epilogue) and waits inside
# migration 2, at its INSERT of the same version: one migration committed, one open.
SUPER_DB=postgres super -c "CREATE DATABASE colophon_held OWNER neon_owner" >/dev/null
held_url="postgres://neon_owner:$OWNER_PASSWORD@127.0.0.1:$PORT/colophon_held"
docker exec -i -e "PGPASSWORD=$OWNER_PASSWORD" "$CONTAINER" psql -h 127.0.0.1 -U neon_owner -d colophon_held -XAtq -v ON_ERROR_STOP=1 -c "
  CREATE TABLE _sqlx_migrations (version BIGINT PRIMARY KEY, description TEXT NOT NULL,
    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(), success BOOLEAN NOT NULL, checksum BYTEA NOT NULL,
    execution_time BIGINT NOT NULL)" >/dev/null
SUPER_DB=colophon_held super -c "BEGIN; INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) VALUES (2, 'held', true, '\\x00', 0); SELECT pg_sleep(60); COMMIT;" >/dev/null 2>&1 &
holder=$!
for _ in $(seq 1 50); do
  [[ "$(SUPER_DB=colophon_held super -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'colophon_held' AND query LIKE '%pg_sleep(60)%' AND state = 'active' AND pid <> pg_backend_pid()")" == 1 ]] && break
  sleep 0.1
done
COLOPHON_MIGRATION_DATABASE_URL="$held_url" "$COLOPHON_BIN" admin migrate >"$work/held.out" 2>&1 &
held_cli=$!
waiting=0
for _ in $(seq 1 300); do
  if [[ "$(SUPER_DB=colophon_held super -c "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid = l.pid WHERE NOT l.granted AND a.datname = 'colophon_held' AND a.usename = 'neon_owner'")" -ge 1 ]]; then waiting=1; break; fi
  sleep 0.1
done
expect "a run held inside its second migration (waiting to record it)" "1" "$waiting"
expect "the first migration committed while the run is held" "1" "$(SUPER_DB=colophon_held super -c "SELECT count(*) FROM _sqlx_migrations WHERE version = 1")"
# held_leftovers: committed CREATE on public in colophon_held, and the owner inheriting colophon_deleter.
held_leftovers() {
  SUPER_DB=colophon_held super -c "
    SELECT coalesce((SELECT string_agg(r.rolname, ',' ORDER BY r.rolname)
                     FROM pg_namespace n, aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) a
                     JOIN pg_roles r ON r.oid = a.grantee
                     WHERE n.nspname = 'public' AND a.privilege_type = 'CREATE' AND r.rolname LIKE 'colophon\_%'), 'none')
           || '|deleter inherited:' || coalesce((SELECT bool_or(m.inherit_option)::text FROM pg_auth_members m
                     WHERE m.roleid = 'colophon_deleter'::regrole AND m.member = 'neon_owner'::regrole), 'none')"
}
expect "while it is held, the committed migration left no grant and the open one shows none" "none|deleter inherited:false" "$(held_leftovers)"
got=$(docker exec -i -e "PGPASSWORD=$APP_PASSWORD" "$CONTAINER" psql -h 127.0.0.1 -U colophon_app -d colophon_held -XAtq 2>&1 <<'SQL'
SET ROLE colophon_delivery;
CREATE FUNCTION public.planted_by_the_app() RETURNS int LANGUAGE sql AS 'SELECT 1';
SQL
)
if grep -q 'permission denied for schema public' <<<"$got"; then
  pass "while it is held, colophon_app (SET ROLE colophon_delivery) can't create in public"
else
  fail "while it is held, colophon_app created in public: ${got:-no error}"
fi
kill -9 "$held_cli" 2>/dev/null || true
wait "$held_cli" 2>/dev/null || true
# The killed run's backend goes with its client; wait for it before looking.
for _ in $(seq 1 50); do
  [[ "$(SUPER_DB=colophon_held super -c "SELECT count(*) FROM pg_stat_activity WHERE datname = 'colophon_held' AND usename = 'neon_owner'")" == 0 ]] && break
  sleep 0.1
done
expect "after the run is killed: no grant left behind" "none|deleter inherited:false" "$(held_leftovers)"
SUPER_DB=postgres super -c "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = 'colophon_held' AND query LIKE '%pg_sleep(60)%'" >/dev/null
wait "$holder" 2>/dev/null || true
code=0
out=$(COLOPHON_MIGRATION_DATABASE_URL="$held_url" "$COLOPHON_BIN" admin migrate 2>&1) || code=$?
if [[ "$code" == 0 ]]; then pass "the next run completes"; else fail "the next run (exit $code): $(tail -3 <<<"$out")"; fi
expect "after the next run: no grant left behind" "none|deleter inherited:false" "$(held_leftovers)"

# Leftovers that got past all that (a hand-made grant, an older build's crash): migrate-status
# reports them, and the next run's sweep removes them.
super -c "GRANT CREATE ON SCHEMA public TO colophon_delivery, colophon_worker" >/dev/null
docker exec -i -e "PGPASSWORD=$OWNER_PASSWORD" "$CONTAINER" psql -h 127.0.0.1 -U neon_owner -d colophon -XAtq -v ON_ERROR_STOP=1 \
  -c "GRANT colophon_deleter TO neon_owner WITH INHERIT TRUE" >/dev/null
cli admin migrate-status --json
expect "migrate-status reports the leftover CREATE on public" '["colophon_delivery","colophon_worker"]' "$(jq -c '.data.grants.create_on_public' <<<"$out" 2>/dev/null)"
expect "migrate-status reports the owner inheriting colophon_deleter" "true" "$(jq -c '.data.grants.deleter_inherited' <<<"$out" 2>/dev/null)"
cli admin migrate
if [[ "$code" == 0 ]]; then pass "admin migrate with leftovers"; else fail "admin migrate with leftovers (exit $code): $(tail -3 <<<"$out")"; fi
grants_after "after a run swept leftovers"
cli admin migrate-status --json
expect "migrate-status after the sweep" '{"create_on_public":[],"deleter_inherited":false}' "$(jq -c '.data.grants' <<<"$out" 2>/dev/null)"

if [[ "$failures" -gt 0 ]]; then
  echo "non-superuser install: $failures of $checks checks failed" >&2
  exit 1
fi
echo "non-superuser install: all $checks checks passed"
