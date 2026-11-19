#!/usr/bin/env bash
# Measures two commits side by side on this machine, alternating before and after for each
# round, and writes one JSON with the machine and both commits (.github/workflows/perf-compare.yml
# runs it on a GitHub runner; the numbers are a CI observation there, never capacity).
#
#   scripts/perf-compare.sh <before> <after> <suite> <rounds> <work directory>
#
# Suites:
#   admission-throughput  crates/colophon-db/tests/admission_throughput.rs at N = 1, 8, 32, 64
#                         (D-216); the harness is taken from <after> when <before> lacks it.
#                         When <after> adds migration 0078, also times it on a seeded database:
#                         its write lock on the work tables is held for that long at upgrade.
#   l08-noisy-agent       scripts/load.sh noisy-agent steady (L08, D-120), each side on a
#                         database of its own; the result files are kept.
#
# Needs the local stack (scripts/stack.sh up), the pinned toolchain, and k6 for L08. Each side is a
# detached worktree under <work directory>, with a target directory of its own.
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
  echo "perf-compare: $*" >&2
  exit 2
}

(($# == 5)) || die "usage: scripts/perf-compare.sh <before> <after> <suite> <rounds> <work directory>"
before_ref=$1
after_ref=$2
suite=$3
rounds=$4
work=$5
[[ $rounds =~ ^[1-5]$ ]] || die "rounds is 1 to 5, got '$rounds'"
case "$suite" in
  admission-throughput | l08-noisy-agent) ;;
  *) die "unknown suite '$suite'" ;;
esac

git fetch --quiet origin '+refs/heads/*:refs/remotes/origin/*'
resolve() {
  local ref=$1 sha
  [[ $ref != -* && $ref =~ ^[A-Za-z0-9._/-]+$ ]] || die "not a ref: '$ref'"
  sha=$(git rev-parse --verify --quiet "$ref^{commit}" || git rev-parse --verify --quiet "origin/$ref^{commit}") ||
    die "no commit for '$ref'"
  echo "$sha"
}
before_sha=$(resolve "$before_ref")
after_sha=$(resolve "$after_ref")

out="$work/out"
mkdir -p "$out"
for side in before after; do
  sha_var="${side}_sha"
  git worktree add --quiet --detach "$work/$side" "${!sha_var}"
done
cpu=$(lscpu | sed -n 's/^Model name: *//p' | head -n1)
cores=$(nproc)
pg=$(docker compose -f deploy/compose.yaml ps -q postgres)
[[ -n $pg ]] || die "the stack's postgres isn't running (scripts/stack.sh up)"
psql_as_owner() { docker exec -i "$pg" psql -U colophon -v ON_ERROR_STOP=1 -q "$@"; }

runs="$out/runs.jsonl"
: >"$runs"
extra='{}'

if [[ $suite == admission-throughput ]]; then
  harness=crates/colophon-db/tests/admission_throughput.rs
  [[ -f "$work/after/$harness" ]] || die "<after> has no $harness"
  [[ -f "$work/before/$harness" ]] || cp "$work/after/$harness" "$work/before/$harness"
  declare -A binary
  for side in before after; do
    echo "building the harness: $side"
    binary[$side]=$(
      cd "$work/$side" &&
        SQLX_OFFLINE=true CARGO_TARGET_DIR="$work/target-$side" \
          cargo test -p colophon-db --test admission_throughput --locked --no-run 2>&1 |
        sed -n 's/^ *Executable .*(\(.*\))$/\1/p' | tail -n1
    )
    [[ -x "$work/$side/${binary[$side]}" || -x "${binary[$side]}" ]] || die "no harness binary for $side"
  done
  for ((round = 1; round <= rounds; round++)); do
    for side in before after; do
      echo "round $round: $side"
      (cd "$work/$side" && "${binary[$side]}" --ignored --exact admissions_a_second_by_concurrent_submitters \
        --nocapture --test-threads=1) | tee "$out/admission-$side-r$round.txt" |
        awk -v round="$round" -v side="$side" '/^N=/ {
          gsub(/= +/, "=")
          line = "{\"round\":" round ",\"side\":\"" side "\""
          for (i = 1; i <= NF; i++) { split($i, kv, "="); line = line ",\"" kv[1] "\":" kv[2] }
          print line "}"; fflush()
        }' >>"$runs"
    done
  done

  migration="$work/after/crates/colophon-db/migrations/0078_payload_counter.sql"
  if [[ -f $migration && ! -f "$work/before/crates/colophon-db/migrations/0078_payload_counter.sql" ]]; then
    echo "timing migration 0078 on a seeded database"
    psql_as_owner -d postgres -c "DROP DATABASE IF EXISTS perf_backfill" -c "CREATE DATABASE perf_backfill"
    for file in "$work/after"/crates/colophon-db/migrations/*.sql; do
      version=$(basename "$file" | sed 's/_.*//')
      ((10#$version < 78)) || continue
      psql_as_owner -d perf_backfill -1 <"$file" >/dev/null
    done
    # Realistic work tables: a million delivery jobs and 200,000 asset jobs, 1% open; 2,000 bulk jobs
    # of 100 items each, 10% open. Seeded with foreign keys and triggers off (replica mode): the
    # migration reads these rows, not the projects they'd belong to.
    psql_as_owner -d perf_backfill <<'SQL'
SET session_replication_role = replica;
INSERT INTO jobs (project_id, environment_id, id, destination_id, generation, idempotency_key,
                  deadline_at, status, finished_at)
SELECT gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), i, 'job:' || i,
       now() + interval '1 hour',
       CASE WHEN i % 100 = 0 THEN 'pending' ELSE 'succeeded' END,
       CASE WHEN i % 100 = 0 THEN NULL ELSE now() END
FROM generate_series(1, 1000000) i;
INSERT INTO assets (project_id, environment_id, id, state, file_name, declared_type, size_bytes,
                    quarantine_key, created_by)
SELECT p, e, a, 'usable', 'a', 'image/png', 1048576, 'quarantine/' || a, gen_random_uuid()
FROM (SELECT gen_random_uuid() p, gen_random_uuid() e, gen_random_uuid() a FROM generate_series(1, 200000)) s;
INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key, deadline_at,
                        status, finished_at)
SELECT project_id, environment_id, gen_random_uuid(), id, 'verify', 'verify:' || id,
       now() + interval '1 hour',
       CASE WHEN row_number() OVER () % 100 = 0 THEN 'pending' ELSE 'succeeded' END,
       CASE WHEN row_number() OVER () % 100 = 0 THEN NULL ELSE now() END
FROM assets;
INSERT INTO bulk_jobs (project_id, environment_id, id, kind, operation, manifest_hash, created_by,
                       principal, expected_items, state, started_at, finished_at)
SELECT gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), 'entries',
       jsonb_build_object('collection', 'articles', 'set', jsonb_build_object('title', repeat('x', 500))),
       decode(repeat('00', 32), 'hex'), gen_random_uuid(), 'member', 100,
       CASE WHEN i % 10 = 0 THEN 'running' ELSE 'done' END, now(),
       CASE WHEN i % 10 = 0 THEN NULL ELSE now() END
FROM generate_series(1, 2000) i;
INSERT INTO bulk_items (project_id, environment_id, job_id, key, entry_id, expected_revision_id, input,
                        outcome, applied_at)
SELECT b.project_id, b.environment_id, b.id, 'k' || k, gen_random_uuid(), gen_random_uuid(),
       jsonb_build_object('title', repeat('y', 200)),
       CASE WHEN b.state = 'running' THEN 'pending' ELSE 'applied' END,
       CASE WHEN b.state = 'running' THEN NULL ELSE now() END
FROM bulk_jobs b, generate_series(1, 100) k;
ANALYZE;
SQL
    rows=$(psql_as_owner -d perf_backfill -At -c \
      "SELECT json_build_object('jobs', (SELECT count(*) FROM jobs), 'asset_jobs', (SELECT count(*) FROM asset_jobs),
              'assets', (SELECT count(*) FROM assets), 'bulk_jobs', (SELECT count(*) FROM bulk_jobs),
              'bulk_items', (SELECT count(*) FROM bulk_items))")
    started=$(date +%s%N)
    psql_as_owner -d perf_backfill -1 <"$migration" >/dev/null
    finished=$(date +%s%N)
    drift=$(psql_as_owner -d perf_backfill -At -c "SELECT drift FROM colophon_payload_reconcile()")
    psql_as_owner -d postgres -c "DROP DATABASE perf_backfill"
    extra=$(jq -cn --argjson rows "$rows" --argjson ms "$(((finished - started) / 1000000))" --argjson drift "$drift" \
      '{migration_0078: {rows: $rows, whole_migration_ms: $ms, drift_after: $drift,
        note: "the whole migration in one transaction: an upper bound on how long it holds writes to the work tables"}}')
  fi
else
  for side in before after; do
    psql_as_owner -d postgres -c "DROP DATABASE IF EXISTS perf_$side" -c "CREATE DATABASE perf_$side"
  done
  for ((round = 1; round <= rounds; round++)); do
    for side in before after; do
      echo "round $round: $side"
      status=0
      (cd "$work/$side" && COLOPHON_TEST_DATABASE="perf_$side" scripts/load.sh noisy-agent steady) >"$out/l08-$side-r$round.log" 2>&1 || status=$?
      result=$(find "$work/$side/perf/results" -name '*noisy-agent-steady*.json' -newer "$runs" -print 2>/dev/null |
        sort | tail -n1)
      if [[ -n $result ]]; then
        cp "$result" "$out/l08-$side-r$round.json"
        jq -c --argjson round "$round" --arg side "$side" --argjson status "$status" \
          '{round: $round, side: $side, exit: $status, result: .}' "$result" >>"$runs"
      else
        jq -cn --argjson round "$round" --arg side "$side" --argjson status "$status" \
          '{round: $round, side: $side, exit: $status, result: null}' >>"$runs"
      fi
      touch "$runs"
    done
  done
fi

jq -n --arg suite "$suite" --arg cpu "$cpu" --argjson cores "$cores" \
  --arg before_ref "$before_ref" --arg before_sha "$before_sha" \
  --arg after_ref "$after_ref" --arg after_sha "$after_sha" --argjson rounds "$rounds" \
  --slurpfile runs "$runs" --argjson extra "$extra" \
  '{label: "CI observation on a shared GitHub runner, not capacity", suite: $suite,
    machine: {cpu: $cpu, cores: $cores}, rounds: $rounds,
    before: {ref: $before_ref, commit: $before_sha}, after: {ref: $after_ref, commit: $after_sha},
    runs: $runs} + $extra' >"$out/perf-compare.json"
echo "wrote $out/perf-compare.json"
