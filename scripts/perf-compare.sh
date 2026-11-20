#!/usr/bin/env bash
# Measures two commits side by side on this machine, alternating before and after for each
# round, and writes one JSON with the machine and both commits (.github/workflows/perf-compare.yml
# runs it on a GitHub runner; the numbers are a CI observation there, never capacity).
#
#   scripts/perf-compare.sh <before> <after> <suite> <rounds> <work directory>
#
# Suites:
#   admission-throughput  crates/colophon-db/tests/admission_throughput.rs at N = 1, 8, 32, 64
#                         (D-216), in both of its modes: across projects and within one
#                         (each run's "mode"); the harness is taken from <after> when <before>
#                         lacks it.
#                         Each migration <after> adds is also timed on a seeded database: its
#                         write lock on the work tables is held for that long at upgrade.
#   l08-noisy-agent       scripts/load.sh noisy-agent steady (L08, D-120), each side on a
#                         database of its own; the result files are kept.
#   smoke                 the smokes job's undo, bulk and assets at its settings (SMOKES
#                         overrides the list), each side on a database of its own, with no
#                         latency gate (the numbers are the point). When pg_stat_statements is
#                         loaded (the workflow turns it on: a restart, so only on a runner's own
#                         stack), each run also keeps its database's top statements by total
#                         time and its work-admission locks by their exact statements as total
#                         ms, calls and mean: WB (installation-wide, gone with D-216) and WA (the
#                         project's), where an acquire's execution time is its wait, and D-216's
#                         counter, colophon_payload_admit() (its row lock's wait plus its fold).
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
  admission-throughput | l08-noisy-agent | smoke) ;;
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
      for mode in across-projects one-project; do
        echo "round $round: $side, $mode"
        test=admissions_a_second_by_concurrent_submitters
        [[ $mode == one-project ]] && test+=_in_one_project
        (cd "$work/$side" && "${binary[$side]}" --ignored --exact "$test" --nocapture --test-threads=1) |
          tee "$out/admission-$side-$mode-r$round.txt" |
          awk -v round="$round" -v side="$side" -v mode="$mode" -f scripts/admission-lines.awk >>"$runs"
      done
    done
  done

  # Each migration <after> adds that <before> lacks, timed on a seeded database: its write lock on
  # the work tables is held for that long at upgrade.
  migrations="$work/after/crates/colophon-db/migrations"
  added=()
  for file in "$migrations"/*.sql; do
    [[ -f "$work/before/crates/colophon-db/migrations/$(basename "$file")" ]] || added+=("$file")
  done
  if ((${#added[@]} > 0)); then
    first=$(basename "${added[0]}" | sed 's/_.*//')
    echo "timing migrations from $first on a seeded database"
    psql_as_owner -d postgres -c "DROP DATABASE IF EXISTS perf_backfill" -c "CREATE DATABASE perf_backfill"
    for file in "$migrations"/*.sql; do
      version=$(basename "$file" | sed 's/_.*//')
      ((10#$version < 10#$first)) || continue
      psql_as_owner -d perf_backfill -1 <"$file" >/dev/null
    done
    # Realistic work tables over 1,000 projects: a million delivery jobs and 200,000 asset jobs, 1%
    # open; 2,000 bulk jobs of 100 items each, 10% open. Seeded with foreign keys and triggers off
    # (replica mode): the migrations read these rows, not the environments they'd belong to.
    psql_as_owner -d perf_backfill <<'SQL'
SET session_replication_role = replica;
INSERT INTO projects (id, slug, name) SELECT gen_random_uuid(), 'perf-' || i, 'Perf' FROM generate_series(1, 1000) i;
INSERT INTO jobs (project_id, environment_id, id, destination_id, generation, idempotency_key,
                  deadline_at, status, finished_at)
SELECT (ARRAY(SELECT id FROM projects ORDER BY id))[1 + i % 1000], gen_random_uuid(), gen_random_uuid(), gen_random_uuid(), i, 'job:' || i,
       now() + interval '1 hour',
       CASE WHEN i % 100 = 0 THEN 'pending' ELSE 'succeeded' END,
       CASE WHEN i % 100 = 0 THEN NULL ELSE now() END
FROM generate_series(1, 1000000) i;
INSERT INTO assets (project_id, environment_id, id, state, file_name, declared_type, size_bytes,
                    quarantine_key, created_by)
SELECT p, e, a, 'usable', 'a', 'image/png', 1048576, 'quarantine/' || a, gen_random_uuid()
FROM (SELECT (ARRAY(SELECT id FROM projects ORDER BY id))[1 + g % 1000] p, gen_random_uuid() e, gen_random_uuid() a
      FROM generate_series(1, 200000) g) s;
INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind, idempotency_key, deadline_at,
                        status, finished_at)
SELECT project_id, environment_id, gen_random_uuid(), id, 'verify', 'verify:' || id,
       now() + interval '1 hour',
       CASE WHEN row_number() OVER () % 100 = 0 THEN 'pending' ELSE 'succeeded' END,
       CASE WHEN row_number() OVER () % 100 = 0 THEN NULL ELSE now() END
FROM assets;
INSERT INTO bulk_jobs (project_id, environment_id, id, kind, operation, manifest_hash, created_by,
                       principal, expected_items, state, started_at, finished_at)
SELECT (ARRAY(SELECT id FROM projects ORDER BY id))[1 + i % 1000], gen_random_uuid(), gen_random_uuid(), 'entries',
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
      "SELECT json_build_object('projects', (SELECT count(*) FROM projects), 'jobs', (SELECT count(*) FROM jobs),
              'asset_jobs', (SELECT count(*) FROM asset_jobs), 'assets', (SELECT count(*) FROM assets),
              'bulk_jobs', (SELECT count(*) FROM bulk_jobs), 'bulk_items', (SELECT count(*) FROM bulk_items))")
    timed='[]'
    for file in "${added[@]}"; do
      started=$(date +%s%N)
      psql_as_owner -d perf_backfill -1 <"$file" >/dev/null
      finished=$(date +%s%N)
      name=$(basename "$file" .sql)
      drift=null
      case "$name" in
        *_payload_counter) drift=$(psql_as_owner -d perf_backfill -At -c "SELECT drift FROM colophon_payload_reconcile()") ;;
        *_open_counter) drift=$(psql_as_owner -d perf_backfill -At -c \
          "SET ROLE colophon_worker; SELECT json_build_object('projects', projects, 'drift', drift) FROM colophon_work_reconcile()" | tail -n1) ;;
      esac
      timed=$(jq -c --arg name "$name" --argjson ms "$(((finished - started) / 1000000))" --argjson drift "$drift" \
        '. + [{migration: $name, whole_migration_ms: $ms, drift_after: $drift}]' <<<"$timed")
    done
    psql_as_owner -d postgres -c "DROP DATABASE perf_backfill"
    extra=$(jq -cn --argjson rows "$rows" --argjson timed "$timed" \
      '{migrations_added: {rows: $rows, timed: $timed,
        note: "each migration whole, in one transaction: an upper bound on how long it holds writes to the work tables"}}')
  fi
elif [[ $suite == smoke ]]; then
  # The smokes job's settings (ci.yml), at reference limits: k6's p99 gate is set out of reach so
  # every run completes and is recorded; hard failures still show in each record.
  smokes=${SMOKES:-undo bulk assets}
  settings_of() {
    case "$1" in
      undo) settings=(TOTAL=8000) ;;
      bulk) settings=(TOTAL=10000) ;;
      assets) settings=("LOAD_LOG=info,colophon_worker=debug" TOTAL=10000) ;;
      *) die "the smoke suite runs undo, bulk or assets, not '$1'" ;;
    esac
  }
  for smoke in $smokes; do settings_of "$smoke"; done
  for side in before after; do
    psql_as_owner -d postgres -c "DROP DATABASE IF EXISTS perf_$side" -c "CREATE DATABASE perf_$side"
  done
  statements=no
  if [[ $(psql_as_owner -d postgres -At -c "SELECT count(*) FROM pg_extension WHERE extname = 'pg_stat_statements'") == 1 ]] &&
    psql_as_owner -d postgres -c "SELECT pg_stat_statements_reset()" >/dev/null 2>&1; then
    statements=yes
  else
    echo "perf-compare: pg_stat_statements isn't loaded: no lock-wait totals (the workflow turns it on)" >&2
  fi
  # pg_stat_statements is the cluster's: a run's rows are its own database's, by oid.
  stats_of() {
    psql_as_owner -d postgres -At -c "
      WITH mine AS (SELECT * FROM pg_stat_statements WHERE dbid = (SELECT oid FROM pg_database WHERE datname = '$1'))
      SELECT json_build_object(
        'top', (SELECT coalesce(json_agg(t), '[]') FROM (
          SELECT round(total_exec_time::numeric, 1) AS total_ms, calls, round(mean_exec_time::numeric, 3) AS mean_ms, rows,
                 left(regexp_replace(query, '\s+', ' ', 'g'), 160) AS query
          FROM mine ORDER BY total_exec_time DESC LIMIT 10) t),
        'locks', (SELECT coalesce(json_agg(l), '[]') FROM (
          SELECT lock, round(sum(total_exec_time)::numeric, 1) AS total_ms, sum(calls) AS calls,
                 round((sum(total_exec_time) / nullif(sum(calls), 0))::numeric, 3) AS mean_ms
          FROM (SELECT *, CASE
                  WHEN query LIKE '%\"held!\" FROM pg_advisory_xact_lock(\$1, \$2)%' THEN 'WB'
                  WHEN query LIKE '%\"held!\" FROM pg_advisory_xact_lock(\$1, hashtext(\$2::text))%' THEN 'WA'
                  WHEN query LIKE '%colophon_payload_admit()%' THEN 'counter'
                END AS lock FROM mine) m
          WHERE lock IS NOT NULL GROUP BY 1 ORDER BY 1) l))"
  }
  for ((round = 1; round <= rounds; round++)); do
    for smoke in $smokes; do
      settings_of "$smoke"
      for side in before after; do
        echo "round $round: $smoke, $side"
        name="$smoke-$side-r$round"
        [[ $statements == yes ]] && psql_as_owner -d postgres -c "SELECT pg_stat_statements_reset()" >/dev/null
        status=0
        (cd "$work/$side" && env "${settings[@]}" KEEP=0 P99_MS=600000 COLOPHON_TEST_DATABASE="perf_$side" \
          RECORD_OUT="$out/$name.record.json" SERVER_LOG="$out/$name.server.log" \
          scripts/load.sh "$smoke" throughput) >"$out/$name.log" 2>&1 || status=$?
        stats=null
        if [[ $statements == yes ]]; then
          stats=$(stats_of "perf_$side") || stats=null
          jq -r '.top[]? | [.total_ms, .calls, .mean_ms, .rows, .query] | @tsv' <<<"$stats" >"$out/$name.statements.txt"
        fi
        record=null
        [[ -s "$out/$name.record.json" ]] && record=$(cat "$out/$name.record.json")
        jq -cn --argjson round "$round" --arg side "$side" --arg smoke "$smoke" --argjson status "$status" \
          --argjson record "$record" --argjson stats "$stats" \
          '{round: $round, side: $side, smoke: $smoke, exit: $status,
            p99_ms: $record.latency_ms.p99, hard_failures_total: $record.hard_failures_total,
            locks: $stats.locks, record: $record}' >>"$runs"
      done
    done
  done
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
