#!/usr/bin/env bash
# Load-tests a release build of colophon-server against the local stack, keeps the evidence,
# and fails on a threshold miss or a regression (D-018).
#
#   scripts/load.sh <endpoint> throughput            1,000,000 requests, back to back (TOTAL, VUS)
#   scripts/load.sh <endpoint> steady                a fixed rate for 60 s (RATE, DURATION, VUS)
#   scripts/load.sh <endpoint> <scenario> --baseline also save this run as the machine's baseline
#
# <endpoint> is a script in perf/k6/: health (the health checks), openapi (the served OpenAPI
# document, alternating full GETs and conditional 304s; D-110), rich-text (validation),
# session (signed-in requests: session lookup and row-level security on every request),
# entries (drafts: reads, lists and If-Match saves, each read checked against the last save),
# content-types (the same for entries of a seven-field type, plus reads of its definition),
# modelling (pages of objects, lists and blocks whose quotes reference people: saves, refused
# references, reads, lists and "referenced by" reads), inverses (a mirrored link moved between
# two writers: every move is a save plus two mirror revisions in one transaction, with mirror
# reads checked against where the link says it is; D-085), transfer (a collection's drafts
# exported as JSONL, nine requests in ten; the tenth imports a three-line file, entries and
# a Strapi entity, in one transaction; D-086), graphql (entries reads through the
# per-collection schema — filters, a sort, a page and one expanded reference per node —
# and one entry read in ten; D-115), locales (a French sibling's save carrying its
# locale, the source's group read, and a locale-filtered list; D-087), deletion (articles created and deleted
# with If-Match, replayed deletions, 404 reads of what was deleted, and deletions refused because
# the article is published, in a changeset or stale; D-080),
# cli (requests authenticated by a bearer token, as the CLI sends them), members (member lists,
# invitation lookups, role changes and new invitations), roles (requests by a member holding a
# custom role, the caller's permissions, role lists and versioned role edits), tokens (requests
# with a read-only and a field-narrowed API token, audit log pages, and tokens made and revoked),
# quotas (two read-only tokens with request ceilings: one inside its budget lists entries,
# one over its two-a-minute ceiling answers 429 with Retry-After; D-118), noisy-agent (L08's
# evidence, D-120: two sequential phases — editorial traffic alone, then the same editorial
# traffic beside a low-ceiling token hammering past its quota and an editor in a second
# project; the run computes and stores whether under-noise editorial p95 held within 2x of
# the baseline it just measured),
# schedules (schedule lists, and scheduling and cancelling a day ahead), projects (who may create
# projects: refused creations and /me), setup (setup codes refused on a set-up installation, and
# /me), changesets (reading, listing, reviewing and creating changesets), stages (the
# environment's ordered review stages: stage-state reads, stage-named reviews and publishes
# refused while a stage waits — the guarded list replace is a config write, proven by tests
# rather than looped; D-104),
# migrations (plan lists and reads, item pages, and planning the same required-field change again:
# a locked dry run of the collection's drafts that supersedes its open plans; D-100),
# collaboration (comment lists and writes on an entry's revision, resolves and reopens on
# comment etags, and task creates and updates on task etags; D-102), live ({P}/events streams
# held open to their clean end while other virtual users save entries steadily — the server
# gets a short COLOPHON_EVENTS_LIFETIME_MS so a hold is seconds, not 30 minutes; D-108),
# delivery (anonymous /content/v1 reads, latest and pinned), publishing (whole publishes, five
# requests each, with the worker announcing every release to a local receiver; needs node),
# the dependency-failure evidence runs (D-127; each pauses a real stack container or kills a
# real process mid-run through the receiver's /control endpoint — run them SERIALLY, never
# beside anything else on the shared stack): cache-down (L11: delivery reads and light
# writes while Valkey is paused then unpaused — every read falls back to Postgres, no
# refusals, bounded fills, hits and latency back after), postgres-trouble (L12: reads and
# writes while Postgres is paused and the separate worker process is SIGKILLed — writes
# fail honestly, no fake commit, /readyz says not_ready, jobs resume), purge-trouble
# (L13: publishes while the receiver answers the purge 503 + Retry-After and the site
# answers a frozen generation — bounded backoff, pending never verified, then drains),
# studio (the Studio's static files; builds the Studio first) or site (the reference site's pages,
# served by its runner while a release is published every two seconds; needs node).
#
# Environment overrides: TOTAL, VUS, RATE, DURATION, P99_MS (the k6 latency threshold),
# KEEP=0 (don't write a result file; for CI smoke runs), LOAD_PORT (default 58090),
# SERVER_LOG=<path> (keep the server's log there); for stress, START_RATE, STEP_SECONDS and
# MAX_STEPS (below); CLUSTER=1 (run against the servers and
# workers scripts/cluster.sh started, through its balancer, instead of one server of its own;
# the result records how many, and is compared only with runs of the same shape: D-075).
#
# RATE and P99_MS default per endpoint. health and rich-text never touch the
# database: 20,000 requests/s and a 25 ms p99. openapi never waits on the database either,
# but each full answer is ~455 KB, so it is bandwidth-bound — a 1,000,000-request throughput
# run on the reference laptop kneed at ~3.5k/s with p99 90 ms at saturation — and its steady
# promise stays well under that knee: 2,000/s and a 100 ms p99. session waits on database round trips, so its steady rate is
# 2,000/s and its p99 threshold 100 ms (END_GOAL's editorial budget is p99 <= 700 ms); the
# baseline comparison still fails a p99 50% worse than the last accepted run. entries writes
# on a fifth of its requests: 500/s steady and a 250 ms p99 (END_GOAL: draft writes p99 <= 700 ms).
# studio reads a file per request: 5,000/s steady (at 10,000/s, rare 75 ms stalls made k6 drop
# 0.04% of its iterations; the result is kept in perf/results/).
#
# A result file goes to perf/results/ and a line to perf/results/history.jsonl. Numbers from
# a laptop are evidence about the code on that machine, not a production capacity claim;
# every result names its machine.
set -euo pipefail
cd "$(dirname "$0")/.."

endpoint="${1:-}"
scenario="${2:-}"
if [[ ! -f "perf/k6/$endpoint.js" ]] || [[ "$scenario" != throughput && "$scenario" != steady && "$scenario" != stress ]]; then
  echo "usage: scripts/load.sh health|rich-text|openapi|session|accounts|cli|members|roles|tokens|quotas|noisy-agent|schedules|projects|setup|changesets|stages|collaboration|live|migrations|delivery|publishing|cache-down|postgres-trouble|purge-trouble|entries|content-types|modelling|inverses|transfer|graphql|locales|deletion|collection-delete|site-starter|assets|bulk|studio|site throughput|steady|stress [--baseline]" >&2
  exit 2
fi
save_baseline=0
[[ "${3:-}" == "--baseline" ]] && save_baseline=1

k6=$(command -v k6 || true)
[[ -n "$k6" ]] || k6="$HOME/.local/bin/k6"
[[ -x "$k6" ]] || { echo "error: k6 not found; run scripts/install-k6.sh" >&2; exit 1; }
for tool in cargo docker jq curl; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done

port="${LOAD_PORT:-58090}"
# The open model needs its virtual users allocated up front: creating them mid-run makes k6
# miss its own schedule (a steady run on 2026-10-25 dropped 271 of 1.2 M iterations at 64).
if [[ "$scenario" == "steady" || "$scenario" == "stress" ]]; then default_vus=256; else default_vus=64; fi
VUS="${VUS:-$default_vus}"
case "$endpoint" in
  # ~455 KB from an in-memory cache: no database, but bandwidth-bound — measured knee
  # ~3.5k/s, so the steady promise is 2,000/s at a 100 ms p99 (D-110).
  openapi) RATE="${RATE:-2000}" P99_MS="${P99_MS:-100}" ;;
  session | projects | setup | delivery | site) RATE="${RATE:-2000}" P99_MS="${P99_MS:-100}" ;;
  # Every request pays Argon2id on the blocking pool (about 50-100 ms of CPU each): the steady
  # rate is deliberately low, and the p99 budget is the hashing queue, not the endpoint's work.
  accounts) RATE="${RATE:-50}" P99_MS="${P99_MS:-500}" ;;
  # Half its requests list entries (a content type lookup too): about 2,000/s at most here, so
  # its steady rate is half that (a run at 2,000/s fell behind; the result is kept).
  cli | members | roles | tokens | schedules | changesets | stages | collaboration) RATE="${RATE:-1000}" P99_MS="${P99_MS:-100}" ;;
  # Eight requests in ten are refused by the request quota (a token's scoped transaction plus a
  # Valkey eval, no handler work); two in ten list entries under a high ceiling: tokens' rate,
  # entries' p99 budget since refusals can still wait on the pool.
  quotas) RATE="${RATE:-1000}" P99_MS="${P99_MS:-250}" ;;
  # Three arms in two phases: ~250/s of editorial requests and ~50/s in a second project while
  # a 60-a-minute token hammers at ~600/s of mostly refused quota checks — quotas' total rate,
  # entries' p99 budget for the same reason.
  noisy-agent) RATE="${RATE:-250}" P99_MS="${P99_MS:-250}" ;;
  # Writers save entries (entries' arm); subscribers' held streams aren't timed — only their
  # clean ends are checked (the p99 gate binds the writes alone).
  live) RATE="${RATE:-500}" P99_MS="${P99_MS:-250}" ;;
  # One request in ten plans a migration: a change-locked dry run of the collection's drafts
  # plus a plan and item insert and a supersede — a heavier write than a changeset's, so half
  # of entries' steady rate and its p99 budget.
  migrations) RATE="${RATE:-250}" P99_MS="${P99_MS:-250}" ;;
  entries | content-types | modelling | site-starter | graphql) RATE="${RATE:-500}" P99_MS="${P99_MS:-250}" ;;
  # A move writes three revisions in one transaction (the post's, and a mirror into each
  # writer): half of entries' steady rate, and the p99 threshold is END_GOAL's editorial
  # write budget — a smoke run on the reference laptop sat at p99 ~580 ms.
  inverses) RATE="${RATE:-250}" P99_MS="${P99_MS:-700}" ;;
  # Nine exports (each reads every draft's content) to one import of three entries (three
  # revision writes in a transaction): near entries' steady rate, but the p99 threshold is
  # END_GOAL's editorial write budget for the import arm.
  transfer) RATE="${RATE:-500}" P99_MS="${P99_MS:-700}" ;;
  # A save carrying its locale, a group read, and a locale-filtered list in about equal
  # measure: inverses' rate, and the p99 threshold is END_GOAL's editorial write budget for
  # the save arm (a revision plus the locale row in one transaction).
  locales) RATE="${RATE:-250}" P99_MS="${P99_MS:-700}" ;;
  # RATE counts iterations here: a create and a deletion, sometimes a read or a replay, or one
  # refused deletion or a list; about 2.3 requests each, three in four of them writes.
  deletion) RATE="${RATE:-200}" P99_MS="${P99_MS:-250}" ;;
  # An iteration is a create and a collection delete, sometimes a replay, or one refused delete
  # or a list: about 2.2 requests each, and a delete scans the other types' reference fields.
  collection-delete) RATE="${RATE:-100}" P99_MS="${P99_MS:-250}" ;;
  # An upload's write is a body-bound check, an S3 PUT and two inserts — transfer's arm: an
  # external call on every write, so half the entries rate and the editorial write budget.
  assets) RATE="${RATE:-250}" P99_MS="${P99_MS:-700}" ;;
  # Job and item reads dominate; one iteration in seven submits a one-item sealed job (the
  # manifest insert plus its two reservations): migrations' rate and p99.
  bulk) RATE="${RATE:-250}" P99_MS="${P99_MS:-250}" ;;
  studio) RATE="${RATE:-5000}" P99_MS="${P99_MS:-25}" ;;
  # RATE counts publishes here, five requests each, and every request writes and commits: on the
  # reference laptop that tops out around 500 to 700 requests a second, so the steady rate is 40
  # publishes (200 requests) a second; a run at 100 fell behind (the result is kept). The p99
  # threshold is END_GOAL's editorial write budget, 700 ms.
  publishing) RATE="${RATE:-40}" P99_MS="${P99_MS:-700}" ;;
  # L11 (D-127): delivery reads at a steady rate while Valkey is paused mid-run — the
  # reads keep their p99 budget because the Postgres fallback is the point, not the pain.
  cache-down) RATE="${RATE:-400}" P99_MS="${P99_MS:-250}" ;;
  # L12 (D-127): reads and light writes while Postgres is paused and the worker is
  # killed; the clean windows carry the entries p99 budget, the outage's own gate is
  # the request deadline.
  postgres-trouble) RATE="${RATE:-100}" P99_MS="${P99_MS:-250}" ;;
  # L13 (D-127): RATE counts publishes (five requests each) through a failing purge
  # sink — publishing's editorial write budget for the p99 gate.
  purge-trouble) RATE="${RATE:-8}" P99_MS="${P99_MS:-700}" ;;
  *) RATE="${RATE:-20000}" P99_MS="${P99_MS:-25}" ;;
esac
base_url="http://127.0.0.1:$port"
public_url="http://localhost:$port"
cluster="${CLUSTER:-0}"
cluster_dir="$PWD/target/cluster"
topology='{"servers": 1, "workers": 0, "balancer": null}'
if [[ "$cluster" == "1" ]]; then
  base_url="http://127.0.0.1:58405"
  public_url="http://localhost:58405"
  servers=$(cat "$cluster_dir/servers" 2>/dev/null || echo 0)
  workers=$(find "$cluster_dir" -name 'worker-*.pid' 2>/dev/null | wc -l | tr -d ' ')
  topology=$(jq -cn --argjson servers "$servers" --argjson workers "$workers" '{servers: $servers, workers: $workers, balancer: "haproxy"}')
fi
work=$(mktemp -d)
server_pid=""
worker_pid=""
receiver_pid=""
site_pid=""
site_port="${SITE_PORT:-58097}"
receiver_port="${RECEIVER_PORT:-58096}"
# A port that already has a listener means a stale process from an earlier run: the
# child spawned here would die on EADDRINUSE while the squatter — with its old state —
# answers in its place. (A stale receiver once drove a real Valkey pause through its
# /control while the run's own receiver.log held only the crash dump, so the warn
# window read empty and every warning counted.) Fail fast, don't measure it.
port_busy() { lsof -nP -t -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }
refuse_squatter() {
  if port_busy "$1"; then
    echo "error: port $1 already has a listener — a stale process would answer in place" >&2
    echo "of this run's own $2. Find it: lsof -nP -iTCP:$1 -sTCP:LISTEN" >&2
    exit 1
  fi
}
# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill -TERM "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  if [[ -n "$receiver_pid" ]]; then
    kill "$receiver_pid" 2>/dev/null || true
    wait "$receiver_pid" 2>/dev/null || true
  fi
  if [[ -n "$site_pid" ]]; then
    kill "$site_pid" 2>/dev/null || true
    wait "$site_pid" 2>/dev/null || true
  fi
  # Never leave the stack dead or the spawned worker behind, however the run ended.
  if [[ -f "$work/worker.pid" ]]; then kill "$(cat "$work/worker.pid" 2>/dev/null)" 2>/dev/null || true; fi
  if [[ "$endpoint" == "cache-down" || "$endpoint" == "postgres-trouble" ]]; then
    docker compose -f deploy/compose.yaml unpause postgres valkey >/dev/null 2>&1 || true
  fi
  if [[ -n "${SERVER_LOG:-}" && -f "$work/server.log" ]]; then cp "$work/server.log" "$SERVER_LOG"; fi
  rm -rf "$work"
}
trap cleanup EXIT

scripts/check-toolchain.sh >/dev/null
scripts/migrate.sh
echo "building the release binaries"
cargo build --quiet --release --locked -p colophon-server -p colophon-cli
studio_dir=""
if [[ "$endpoint" == "studio" ]]; then
  echo "building the Studio"
  npm run build --silent -w @colophon/studio >/dev/null
  studio_dir="$PWD/apps/studio/build"
fi

# postgres-trouble splits roles: the API server runs --role api and a separate
# --role worker process runs the outbox, so the scenario's SIGKILL can take the worker
# down without taking the API with it (L12, D-127). The receiver's /control kills and
# respawns it through WORKER_PID_FILE and WORKER_CMD; a 15 s lease (config floor is
# above the 10 s attempt deadline) lets a killed claim lapse inside the recovery window.
worker_port="${WORKER_PORT:-58092}"
worker_cmd=""
if [[ "$endpoint" == "postgres-trouble" && "$cluster" != "1" ]]; then
  worker_cmd="cd '$PWD' && . scripts/local-env.sh && export COLOPHON_ROLE=worker COLOPHON_LISTEN_ADDR=127.0.0.1:$worker_port COLOPHON_PUBLIC_URL=http://localhost:$worker_port COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json COLOPHON_WORKER_LEASE_MS=${COLOPHON_WORKER_LEASE_MS:-15000} && exec target/release/colophon-server >>'$work/worker.log' 2>&1"
fi

# The publishing and dependency-failure scenarios' destination — and, for the failure
# ones, the fault driver (its /control endpoint pauses the real stack container, flips
# the purge and site modes, and kills/respawns the worker; D-127). The worker env lets
# the receiver find and respawn the postgres-trouble worker.
if [[ "$endpoint" == "publishing" || "$endpoint" == "cache-down" || "$endpoint" == "postgres-trouble" || "$endpoint" == "purge-trouble" ]]; then
  command -v node >/dev/null || { echo "error: $endpoint needs node" >&2; exit 1; }
  refuse_squatter "$receiver_port" "fault receiver"
  if [[ -n "$worker_cmd" ]]; then
    WORKER_PID_FILE="$work/worker.pid" WORKER_CMD="$worker_cmd" \
      node perf/receiver/receiver.mjs "$receiver_port" >"$work/receiver.log" 2>&1 &
  else
    node perf/receiver/receiver.mjs "$receiver_port" >"$work/receiver.log" 2>&1 &
  fi
  receiver_pid=$!
  # The bind can still lose a race after the squatter check; a dead receiver must not
  # be discovered by the run's missing fault stamps.
  sleep 0.5
  if ! kill -0 "$receiver_pid" 2>/dev/null; then
    echo "error: the fault receiver died at startup:" >&2
    cat "$work/receiver.log" >&2
    exit 1
  fi
fi

# The site scenario's runner: builds each release it hears about and serves it (apps/web). It
# adopts the project the scenario creates from the first announcement.
if [[ "$endpoint" == "site" ]]; then
  command -v node >/dev/null || { echo "error: the site scenario needs node" >&2; exit 1; }
  refuse_squatter "$site_port" "site runner"
  SITE_API="$base_url" SITE_ROOT="$work/site" SITE_LISTEN="127.0.0.1:$site_port" \
    node apps/web/src/runner.ts >"$work/site.log" 2>&1 &
  site_pid=$!
fi

# How much each cluster process had logged before the run: only what the run adds counts.
cluster_log_sizes() {
  for file in "$cluster_dir"/*.log; do
    [[ -e "$file" ]] && printf '%s %s\n' "$file" "$(wc -c <"$file" | tr -d ' ')"
  done
}
if [[ "$cluster" == "1" ]]; then
  if ! target/release/colophon doctor --server "$base_url" --timeout-ms 1000 >/dev/null 2>&1; then
    echo "error: CLUSTER=1, but the balancer isn't ready (scripts/cluster.sh up)" >&2
    exit 1
  fi
  cluster_log_sizes >"$work/log-sizes"
else
  # A squatter on the API port would pass the readiness probe below and the run would
  # measure it; on the worker port, the respawned worker would never bind.
  refuse_squatter "$port" "API server"
  [[ -z "$worker_cmd" ]] || refuse_squatter "$worker_port" "worker"
  # The load-test server: the local stack's settings, its own port, and warnings-only logging,
  # as production would run (per-request lines are DEBUG).
  (
    # shellcheck source=scripts/local-env.sh
    . scripts/local-env.sh
    # The public URL is the sign-in redirect URI registered in the development realm.
    export COLOPHON_LISTEN_ADDR="127.0.0.1:$port" COLOPHON_PUBLIC_URL="http://localhost:$port" COLOPHON_LOG=warn COLOPHON_LOG_FORMAT=json
    # A load run can hold hundreds of live sockets, and a big response keeps each one open
    # longer: under this Mac's 256-file default the accept loop hit EMFILE about once a second
    # on GET /openapi.json at 2,000/s (D-110's scenario). Production runs under a service
    # manager's raised limit, so the measured server gets one too — the run measures the
    # endpoint, not macOS's default.
    ulimit -n "${COLOPHON_LOAD_MAXFILES:-16384}" 2>/dev/null || true
    # One machine sends every request: rate limits (D-076) that never refuse, so each request
    # still pays for its check and the run measures it.
    export COLOPHON_RATE_LIMIT_PER_ADDRESS="${COLOPHON_RATE_LIMIT_PER_ADDRESS:-1000000/1000000}" \
      COLOPHON_RATE_LIMIT_PER_CREDENTIAL="${COLOPHON_RATE_LIMIT_PER_CREDENTIAL:-1000000/1000000}" \
      COLOPHON_RATE_LIMIT_DELIVERY="${COLOPHON_RATE_LIMIT_DELIVERY:-1000000/1000000}"
    if [[ -n "$studio_dir" ]]; then export COLOPHON_STUDIO_DIR="$studio_dir"; fi
    # The live scenario's subscribers hold each stream to its clean end: a 15-second lifetime
    # keeps a hold in seconds (the 30-minute default is past the client's 90-second timeout).
    if [[ "$endpoint" == "live" ]]; then export COLOPHON_EVENTS_LIFETIME_MS="${COLOPHON_EVENTS_LIFETIME_MS:-15000}"; fi
    # postgres-trouble runs the API and the outbox apart, so the scenario's worker kill
    # doesn't take the API down with it (L12, D-127).
    if [[ "$endpoint" == "postgres-trouble" ]]; then export COLOPHON_ROLE=api; fi
    exec target/release/colophon-server
  ) >"$work/server.log" 2>&1 &
  server_pid=$!

  ready=0
  for _ in $(seq 1 100); do
    if target/release/colophon doctor --server "$base_url" --timeout-ms 500 >/dev/null 2>&1; then
      ready=1
      break
    fi
    sleep 0.2
  done
  if (( ready == 0 )); then
    echo "error: the server never became ready (is the stack up? scripts/stack.sh up)" >&2
    target/release/colophon doctor --server "$base_url" >&2 || true
    cat "$work/server.log" >&2
    exit 1
  fi

  # The postgres-trouble worker: the same env the receiver's /control uses to respawn it.
  if [[ -n "$worker_cmd" ]]; then
    bash -c "$worker_cmd" &
    worker_pid=$!
    echo "$worker_pid" >"$work/worker.pid"
  fi
fi

# The database the server uses: `colophon`, or COLOPHON_TEST_DATABASE's (scripts/local-env.sh
# checked it is a plain name).
database="${COLOPHON_TEST_DATABASE:-colophon}"
db_transactions() {
  docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -XAtqc \
    "select xact_commit + xact_rollback from pg_stat_database where datname = '$database'"
}
sleep 1.5 # let Postgres publish its statistics
tx_before=$(db_transactions)

# Stress (milestone S, D-077): the steady scenario at rising rates on one server (or the
# cluster), until a step breaks its own thresholds. Each step runs STEP_SECONDS (default 20) at
# a rate 25% above the last, from START_RATE (default a quarter of the endpoint's RATE), at most
# MAX_STEPS (default 15). The knee is the highest rate that held; the failure mode is how the
# first step that didn't hold broke: shed load (every failed request the servers logged was 503
# `unavailable` at WARN, which only a request that waited out the pool's acquire timeout logs,
# D-033: overload handled as designed), errors (anything else), falling behind (k6 couldn't
# start iterations on time because answers were too slow), or latency over P99_MS. Each step
# keeps the WARN and ERROR lines it added to the servers' logs: failed requests by code (or a
# middleware failure's status), other lines by message, so an error is never only a number. A step that breaks is run once
# more, and only breaking twice ends the climb. A measurement, not a pass/fail gate: it exits 0
# unless the server is unhealthy afterwards.
if [[ "$scenario" == "stress" ]]; then
  step_seconds="${STEP_SECONDS:-20}"
  rate="${START_RATE:-$(( RATE / 4 ))}"
  (( rate >= 10 )) || rate=10
  max_steps="${MAX_STEPS:-15}"
  steps='[]'
  knee=0
  failure="none within $max_steps steps"
  echo "stressing $endpoint from $rate/s, ${step_seconds} s a step, p99 gate $P99_MS ms"
  # Where each server's log ends now, so a step counts only the lines it adds.
  log_sizes() {
    if [[ "$cluster" == "1" ]]; then
      cluster_log_sizes
    else
      printf '%s %s\n' "$work/server.log" "$(wc -c <"$work/server.log" | tr -d ' ')"
    fi
  }
  # The WARN and ERROR lines added since log_sizes wrote $work/step-log-sizes: failed requests
  # by code (`unavailable`, `internal`, ...) or a middleware failure's status, the rest (such as
  # sqlx's slow-acquire warnings) by message.
  logged_failures() {
    while read -r file size; do
      tail -c +"$(( size + 1 ))" "$file"
    done <"$work/step-log-sizes" | jq -Rsc '
      def counts: group_by(.) | map({key: .[0], value: length}) | from_entries;
      [split("\n")[] | fromjson? | select(.level == "WARN" or .level == "ERROR")]
      | {requests: (map(select(.code or .status) | (.code // .status)) | counts),
         other: (map(select((.code or .status) | not) | (.message // "unknown")) | counts)}'
  }
  # One step at the current rate: appends it to $steps, sets $entry and $step_status.
  run_step() {
    local attempt=$1 load_average logged
    load_average=$(uptime | sed 's/.*averages*: //' | awk '{print $1}' | tr -d ,)
    log_sizes >"$work/step-log-sizes"
    step_status=0
    "$k6" run --quiet --no-color --summary-export "$work/step.json" \
      -e BASE_URL="$base_url" -e PUBLIC_URL="$public_url" -e SCENARIO=steady \
      -e VUS="$VUS" -e RATE="$rate" -e DURATION="${step_seconds}s" \
      -e P99_MS="$P99_MS" -e RECEIVER_URL="http://127.0.0.1:$receiver_port" -e SITE_URL="http://127.0.0.1:$site_port" \
      "perf/k6/$endpoint.js" >"$work/step.log" 2>&1 || step_status=$?
    logged=$(logged_failures)
    entry=$(jq -c --argjson rate "$rate" --argjson attempt "$attempt" --argjson status "$step_status" --arg load "$load_average" \
      --argjson logged "$logged" '{
      rate: $rate, attempt: $attempt, rps: (.metrics.http_reqs.rate | floor),
      p99_ms: (.metrics.http_req_duration."p(99)" * 1000 | round / 1000),
      failed: (.metrics.http_req_failed.passes // 0), checks_failed: (.metrics.checks.fails // 0),
      dropped: (.metrics.dropped_iterations.count // 0), held: ($status == 0), load_average: ($load | tonumber? // null),
      server_logged: $logged
    }' "$work/step.json")
    steps=$(jq -c --argjson entry "$entry" '. + [$entry]' <<<"$steps")
    echo "  $(jq -r '"\(.rate)/s: \(.rps) requests/s, p99 \(.p99_ms) ms, \(.failed) failed, \(.dropped) dropped: " + (if .held then "held" else "broke" end)
      + (if .server_logged.requests == {} then "" else " (failed requests the servers logged: \(.server_logged.requests | to_entries | map("\(.value) \(.key)") | join(", ")))" end)' <<<"$entry")"
  }
  for _ in $(seq 1 "$max_steps"); do
    run_step 1
    # One broken window can be a passing stall (a checkpoint, another process): a step breaks
    # only if it breaks twice in a row (a first run on 2026-10-28 "broke" at 303/s on one
    # p99 spike, and held to 731/s when run again).
    if (( step_status != 0 )); then
      echo "  once more, to tell a stall from a limit"
      run_step 2
    fi
    if (( step_status != 0 )); then
      failure=$(jq -r --argjson gate "$P99_MS" '
        if .failed > 0 and (.server_logged.requests | keys) == ["unavailable"] and .server_logged.requests.unavailable >= .failed then "shed load"
        elif .failed > 0 or .checks_failed > 0 then "errors"
        elif .dropped > 0 then "fell behind"
        elif .p99_ms > $gate then "latency"
        else "thresholds" end' <<<"$entry")
      break
    fi
    knee=$rate
    rate=$(( rate * 5 / 4 ))
  done
  healthy=true
  target/release/colophon doctor --server "$base_url" >/dev/null || healthy=false
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string)
    cores=$(sysctl -n hw.ncpu)
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
    cores=$(nproc)
  fi
  machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
  shape=""
  [[ "$cluster" == "1" ]] && shape="-cluster$(echo "$topology" | jq -r '"\(.servers)s\(.workers)w"')"
  stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  record=$(jq -cn --arg endpoint "$endpoint" --arg stamp "$stamp" --arg commit "$(git rev-parse --short HEAD)" \
    --argjson dirty "$([[ -n "$(git status --porcelain -- crates Cargo.lock perf/k6)" ]] && echo true || echo false)" \
    --arg machine "$machine" --arg cpu "$cpu" --argjson cores "$cores" --argjson topology "$topology" \
    --argjson gate "$P99_MS" --argjson step_seconds "$step_seconds" --argjson steps "$steps" \
    --argjson knee "$knee" --arg failure "$failure" --argjson healthy "$healthy" \
    '{scenario: "stress", endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty,
      machine: {id: $machine, cpu: $cpu, cores: $cores}, topology: $topology,
      p99_gate_ms: $gate, step_seconds: $step_seconds, knee_rate: $knee, failure_mode: $failure,
      healthy_after: $healthy, steps: $steps}')
  echo "$record" | jq '{endpoint, knee_rate, failure_mode, healthy_after, topology}'
  if [[ "${KEEP:-1}" == "1" ]]; then
    mkdir -p perf/results
    file="perf/results/${stamp//:/}-$endpoint-stress-$machine$shape.json"
    echo "$record" | jq . >"$file"
    echo "$record" >>perf/results/history.jsonl
    echo "saved $file"
  fi
  [[ "$healthy" == true ]]
  exit
fi

echo "running k6: $endpoint $scenario"
k6_status=0
"$k6" run --quiet --no-color \
  --summary-export "$work/summary.json" \
  -e BASE_URL="$base_url" -e PUBLIC_URL="$public_url" -e SCENARIO="$scenario" \
  -e TOTAL="${TOTAL:-1000000}" -e VUS="$VUS" \
  -e RATE="$RATE" -e DURATION="${DURATION:-60s}" \
  -e P99_MS="$P99_MS" -e RECEIVER_URL="http://127.0.0.1:$receiver_port" -e SITE_URL="http://127.0.0.1:$site_port" \
  "perf/k6/$endpoint.js" || k6_status=$?

# A scenario that drove a real outage must have healed it — but even a crashed run
# doesn't get to leave the stack dead for the checks below.
if [[ "$endpoint" == "cache-down" || "$endpoint" == "postgres-trouble" ]]; then
  docker compose -f deploy/compose.yaml unpause postgres valkey >/dev/null 2>&1 || true
fi

sleep 1.5
tx_after=$(db_transactions)

# The server must still be healthy after the run, and must not have logged any warnings
# outside the injected outage's window. A healed dependency's readiness probes take a
# few seconds to report up again, so the failure scenarios' check retries briefly.
post_run_ready=0
for _ in $(seq 1 15); do
  target/release/colophon doctor --server "$base_url" >/dev/null 2>&1 && post_run_ready=1 && break
  [[ "$endpoint" != "cache-down" && "$endpoint" != "postgres-trouble" ]] && break
  sleep 1
done
if (( post_run_ready == 0 )); then
  echo "error: the server is not ready after the run" >&2
  k6_status=1
fi
if [[ "$cluster" == "1" ]]; then
  server_warnings=0
  while read -r file size; do
    added=$(tail -c +"$(( size + 1 ))" "$file" | grep -c '"level":"\(WARN\|ERROR\)"' || true)
    server_warnings=$(( server_warnings + added ))
  done <"$work/log-sizes"
elif [[ "$endpoint" == "cache-down" || "$endpoint" == "postgres-trouble" ]]; then
  # The injected outage explains warnings inside its window — a paused dependency
  # makes the readiness probes log their transitions and every starved request its
  # 503 at WARN (D-033), and that is the evidence, not a defect. The receiver stamps
  # `fault <dep> down|up <iso>` around each docker call, so WARN/ERROR lines stamped
  # between `down` and `up` (plus a short grace for stragglers) are the outage doing
  # its job; anything outside the window still fails the run. No stamp, no cover.
  down_at=$(awk '$1=="fault" && ($2=="postgres" || $2=="valkey") && $3=="down" {print $4; exit}' "$work/receiver.log" 2>/dev/null || true)
  up_at=$(awk '$1=="fault" && ($2=="postgres" || $2=="valkey") && $3=="up" {t=$4} END {print t}' "$work/receiver.log" 2>/dev/null || true)
  to_epoch() { jq -rn --arg t "$1" '($t[0:19] + "Z") | fromdateiso8601' 2>/dev/null || echo ""; }
  warn_logs=("$work/server.log")
  [[ -f "$work/worker.log" ]] && warn_logs+=("$work/worker.log")
  lo=""; hi=""
  if [[ -n "$down_at" ]]; then ep=$(to_epoch "$down_at"); [[ "$ep" =~ ^[0-9]+$ ]] && lo=$(( ep - 2 )); fi
  # The window's far edge covers requests still in flight at the heal: one issued in its last
  # second can log its failure at up + the 10 s request deadline (plus a beat).
  if [[ -n "$up_at" ]]; then ep=$(to_epoch "$up_at"); [[ "$ep" =~ ^[0-9]+$ ]] && hi=$(( ep + 12 )); fi
  [[ -z "$hi" ]] && hi=$(date +%s)
  if [[ -n "$lo" ]]; then
    warnings_outside=$(cat "${warn_logs[@]}" | jq -Rsc \
      --argjson lo "$lo" --argjson hi "$hi" '
      def epoch: try ((.timestamp[0:19] + "Z") | fromdateiso8601) catch -1;
      [split("\n")[] | fromjson? | select(.level == "WARN" or .level == "ERROR")
       | select(epoch == -1 or epoch < $lo or epoch > $hi)]')
  else
    warnings_outside=$(cat "${warn_logs[@]}" | jq -Rsc '[split("\n")[] | fromjson? | select(.level == "WARN" or .level == "ERROR")]')
  fi
  server_warnings=$(jq 'length' <<<"$warnings_outside")
else
  server_warnings=$(grep -c '"level":"\(WARN\|ERROR\)"' "$work/server.log" || true)
fi

commit=$(git rev-parse --short HEAD)
dirty=$([[ -n "$(git status --porcelain -- crates Cargo.lock perf/k6)" ]] && echo true || echo false)
if [[ "$(uname -s)" == "Darwin" ]]; then
  cpu=$(sysctl -n machdep.cpu.brand_string)
  cores=$(sysctl -n hw.ncpu)
  memory_gb=$(( $(sysctl -n hw.memsize) / 1024 / 1024 / 1024 ))
else
  cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')
  cores=$(nproc)
  memory_gb=$(( $(grep MemTotal /proc/meminfo | awk '{print $2}') / 1024 / 1024 ))
fi
machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
# Runs through the balancer are their own series: file names and baselines carry the shape.
shape=""
[[ "$cluster" == "1" ]] && shape="-cluster$(echo "$topology" | jq -r '"\(.servers)s\(.workers)w"')"
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)

record=$(jq -c \
  --arg scenario "$scenario" --arg endpoint "$endpoint" --arg stamp "$stamp" --arg commit "$commit" --argjson dirty "$dirty" \
  --arg cpu "$cpu" --argjson cores "$cores" --argjson memory_gb "$memory_gb" --arg machine "$machine" \
  --arg os "$(uname -s) $(uname -r)" --arg k6 "$("$k6" version | awk '{print $2}')" \
  --argjson tx "$(( tx_after - tx_before ))" --argjson warnings "$server_warnings" \
  --argjson k6_status "$k6_status" --argjson topology "$topology" \
  --arg total "${TOTAL:-1000000}" --arg vus "$VUS" --arg rate "$RATE" --arg duration "${DURATION:-60s}" \
  '{
    scenario: $scenario, endpoint: $endpoint, date: $stamp, commit: $commit, dirty: $dirty,
    machine: { id: $machine, cpu: $cpu, cores: $cores, memory_gb: $memory_gb, os: $os },
    topology: $topology,
    k6: $k6,
    params: (if $scenario == "throughput" then { total: ($total | tonumber), vus: ($vus | tonumber) }
             else { rate: ($rate | tonumber), duration: $duration, vus: ($vus | tonumber) } end),
    requests: .metrics.http_reqs.count,
    rps: (.metrics.http_reqs.rate | floor),
    failed: (.metrics.http_req_failed.passes // 0),
    checks_failed: (.metrics.checks.fails // 0),
    dropped: (.metrics.dropped_iterations.count // 0),
    latency_ms: (.metrics.http_req_duration | {
      avg: (.avg * 1000 | round / 1000), p50: (.med * 1000 | round / 1000),
      p90: (."p(90)" * 1000 | round / 1000), p95: (."p(95)" * 1000 | round / 1000),
      p99: (."p(99)" * 1000 | round / 1000), p999: (."p(99.9)" * 1000 | round / 1000),
      max: (.max * 1000 | round / 1000) }),
    database_transactions_during_run: $tx,
    server_warnings: $warnings,
    thresholds_passed: ($k6_status == 0)
  } + (if .l08 then {l08: .l08} else {} end)
    + (if .l11 then {l11: .l11} else {} end)
    + (if .l12 then {l12: .l12} else {} end)
    + (if .l13 then {l13: .l13} else {} end)' "$work/summary.json")

echo "$record" | jq .

if [[ "${KEEP:-1}" == "1" ]]; then
  mkdir -p perf/results perf/baselines
  file="perf/results/${stamp//:/}-$endpoint-$scenario-$machine$shape.json"
  echo "$record" | jq . >"$file"
  echo "$record" >>perf/results/history.jsonl
  echo "saved $file"
fi

status=$k6_status
if (( server_warnings > 0 )); then
  echo "FAIL: the server logged $server_warnings warning(s) or error(s) under load" >&2
  # The lines, so a real fault-window miss is diagnosable from the output, not just a count.
  if declare -p warnings_outside >/dev/null 2>&1; then
    if [[ -n "$down_at" ]]; then
      echo "  fault window was down $down_at up ${up_at:-never}" >&2
    else
      echo "  no 'fault <dep> down' stamp in the receiver's log — no window, every warning counts" >&2
    fi
    jq -r '.[:10][] | tojson' <<<"$warnings_outside" >&2 || true
  fi
  status=1
fi

# The lXX gates are comparisons only the run can compute (perf/k6/noisy-agent.js measures
# both phases; the dependency-failure scenarios measure their own outage windows): each
# exported summary's object carries the numbers and the verdict — a run whose `held`
# is false fails here, the numbers beside it in the result file.
for gate in l08 l11 l12 l13; do
  # `// empty` would swallow a held:false (jq's // treats false like null) — read it raw:
  # the gate must fire on a computed false and stay quiet when the key is absent.
  if [[ "$(jq -r ".$gate.held" "$work/summary.json")" == "false" ]]; then
    echo "FAIL: the ${gate} gate did not hold: $(jq -c ".$gate" "$work/summary.json")" >&2
    status=1
  fi
done

baseline="perf/baselines/$endpoint-$scenario-$machine$shape.json"
# Only like with like: a 10,000-request smoke spends a visible share of its requests opening
# pool connections and preparing statements, which a million-request run amortizes away, so
# a run with different parameters relies on its absolute thresholds alone.
if [[ -f "$baseline" ]] && ! jq -e --argjson now "$record" '.params == $now.params' "$baseline" >/dev/null; then
  echo "not compared with $baseline: different parameters ($(echo "$record" | jq -c .params) vs $(jq -c .params "$baseline")); absolute thresholds only"
elif [[ -f "$baseline" ]]; then
  # A regression: throughput 25% lower, or p99 50% higher (plus 1 ms, so microsecond noise on
  # a tiny p99 doesn't fail the run). The baseline is per machine; never compare across them.
  verdict=$(jq -n --argjson now "$record" --slurpfile base "$baseline" '
    $base[0] as $b |
    [ (if $now.scenario == "throughput" and $now.rps < ($b.rps * 0.75) then "throughput \($now.rps)/s is below 75% of the baseline \($b.rps)/s" else empty end),
      (if $now.latency_ms.p99 > ($b.latency_ms.p99 * 1.5 + 1) then "p99 \($now.latency_ms.p99) ms is above 1.5x the baseline \($b.latency_ms.p99) ms + 1 ms" else empty end)
    ] | join("; ")')
  verdict=${verdict//\"/}
  if [[ -n "$verdict" ]]; then
    echo "FAIL: regression against $baseline: $verdict" >&2
    status=1
  else
    echo "no regression against $baseline"
  fi
fi
if (( save_baseline == 1 )); then
  if (( status == 0 )); then
    echo "$record" | jq . >"$baseline"
    echo "saved the baseline $baseline"
  else
    echo "not saving a failed run as the baseline" >&2
  fi
fi

if (( status == 0 )); then
  echo "load test passed: $endpoint $scenario, $(echo "$record" | jq -r '"\(.requests) requests, \(.rps)/s, p99 \(.latency_ms.p99) ms, \(.failed) failed, \(.database_transactions_during_run) database transactions"')"
fi
exit "$status"
