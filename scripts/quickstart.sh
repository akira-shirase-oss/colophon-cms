#!/usr/bin/env bash
# The quick start, run exactly as documented (D-172, D-173): deploy/quickstart/compose.yaml in
# an empty directory, `docker compose up -d --wait`, the Studio and the API answering, the
# setup code read from the logs with the command the Studio shows, and then the checks a
# newcomer can't see: no dependency reachable from the host, password authentication on
# loopback, no secret in any container's environment, data kept across `down` and `up`.
#
#   scripts/quickstart.sh [--image IMAGE] [--port PORT] [--browser] [--load] [--record] [--keep]
#
#   --image    the Colophon image (default colophon:local: docker build -f deploy/image/Dockerfile -t colophon:local .)
#   --port     the host port (default 8080)
#   --browser  also run the Studio's Playwright journey (apps/studio, playwright.quickstart.config.ts)
#   --load     also run the k6 smoke (perf/k6/quickstart.js: 200 requests a second for 30 s)
#   --record   write the timings and sizes to perf/results/<stamp>-quickstart-<machine>.json
#   --keep     leave the containers and volumes running afterwards
#
# Times reported: `up --wait` from nothing to ready (images already present), the same after
# `down` (a restart with data), and the Colophon container alone from start to healthy (its
# cold start). Sizes: the image as stored, and as pulled (gzip of `docker save`).
# Needs Docker with Compose 2.24 or newer, curl and jq.
set -euo pipefail
cd "$(dirname "$0")/.."
repo=$PWD

image=colophon:local
port=8080
browser=false
load=false
record=false
keep=false
while [[ $# -gt 0 ]]; do
  case $1 in
    --image) image=$2; shift 2 ;;
    --port) port=$2; shift 2 ;;
    --browser) browser=true; shift ;;
    --load) load=true; shift ;;
    --record) record=true; shift ;;
    --keep) keep=true; shift ;;
    *) echo "usage: scripts/quickstart.sh [--image IMAGE] [--port PORT] [--browser] [--load] [--record] [--keep]" >&2; exit 2 ;;
  esac
done

# A project of its own, so a quick start someone already runs is never touched.
export COMPOSE_PROJECT_NAME="colophon-quickstart-check-$port"
export COLOPHON_IMAGE=$image COLOPHON_PORT=$port
origin="http://localhost:$port"

work=$(mktemp -d "${TMPDIR:-/tmp}/colophon-quickstart.XXXXXX")
cp deploy/quickstart/compose.yaml "$work/compose.yaml"
compose() { (cd "$work" && docker compose "$@"); }

cleanup() {
  local rc=$?
  if [[ $rc != 0 ]]; then
    echo "--- the services' logs, for the failure above"
    compose logs --no-color --tail 60 2>/dev/null || true
  fi
  if [[ $keep == false ]]; then compose down --volumes --timeout 10 >/dev/null 2>&1 || true; fi
  rm -rf "$work"
}
trap cleanup EXIT

failures=0
pass() { printf '  ok    %s\n' "$1"; }
fail() { failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }
check() { if eval "$2" >/dev/null 2>&1; then pass "$1"; else fail "$1"; fi; }
now() { python3 -c 'import time; print(f"{time.time():.3f}")' 2>/dev/null || date +%s; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", b - a }'; }

echo "==> docker compose config"
compose config --quiet

echo "==> docker compose up -d --wait (first start: secrets, roles, schema, bucket)"
t0=$(now)
compose up -d --wait --wait-timeout 300
first_up=$(elapsed "$t0" "$(now)")
echo "    ready in ${first_up}s"

echo "==> what a newcomer sees"
check "colophon doctor says ready" "compose exec -T colophon /usr/local/bin/colophon doctor --server http://127.0.0.1:8080"
check "the Studio answers at $origin/studio/" "curl -fsS '$origin/studio/' | grep -qi '<html'"
installation=$(curl -fsS "$origin/api/v1/installation")
check "the API says nobody runs it yet, from a container" \
  "jq -e '.data == {has_administrator: false, install_kind: \"container\"}' <<<'$installation'"
check "/version names the image's build" "curl -fsS '$origin/version' | jq -e '.version | length > 0'"
code=$(compose logs colophon 2>/dev/null | grep "Setup code" | sed -n 's/.*Setup code: \([0-9A-Z-]*\).*/\1/p' | tail -n 1)
check "the setup code is in docker compose logs colophon" "[[ '$code' =~ ^[0-9A-Z]{5}(-[0-9A-Z]{5}){3}\$ ]]"

echo "==> what a newcomer can't see"
check "only Colophon's port is published, on 127.0.0.1" \
  "[[ \$(docker port \$(compose ps -q pod)) == '8080/tcp -> 127.0.0.1:$port' ]]"
# From another container on the project's network: Colophon answers, its dependencies don't
# (they listen on the shared namespace's loopback only).
network=$(docker inspect -f '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}' "$(compose ps -q pod)")
probe() { docker run --rm --network "$network" alpine:3.23@sha256:469c9ea410b9215eaee7976ba6c0639039b444be8e5d638f7abbada189971eb2 nc -z -w 2 pod "$1"; }
check "a neighbouring container reaches Colophon on 8080" "probe 8080"
for dependency in 5432 6379 3900 3901 3903; do
  check "a neighbouring container can't reach port $dependency" "! probe $dependency"
done
check "PostgreSQL takes the generated password on loopback" \
  "compose exec -T postgres sh -c 'PGPASSWORD=\$(cat /run/secrets/postgres/owner_password) psql -h 127.0.0.1 -U colophon -d colophon -c \"select 1\"'"
check "PostgreSQL refuses a wrong password on loopback" \
  "! compose exec -T postgres psql 'postgres://colophon:wrong@127.0.0.1:5432/colophon' -c 'select 1'"
check "PostgreSQL refuses a wrong password on its socket too" \
  "! compose exec -T postgres sh -c 'PGPASSWORD=wrong psql -U colophon -d colophon -c \"select 1\"'"
secrets=$(compose run --rm --no-deps -T --entrypoint sh secrets -c 'cat /secrets/*/* 2>/dev/null' | grep -Ev '^(bind|port|protected-mode|save|appendonly|maxmemory)' | sed 's/^requirepass //; s/.*:\/\/[^:]*:\([^@]*\)@.*/\1/')
leaked=0
for container in $(compose ps -aq); do
  environment=$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}{{range .Config.Cmd}}{{println .}}{{end}}' "$container")
  while IFS= read -r secret; do
    [[ ${#secret} -ge 16 ]] || continue
    if grep -qF -- "$secret" <<<"$environment"; then leaked=$((leaked + 1)); fi
  done <<<"$secrets"
done
generated=$(grep -c . <<<"$secrets")
check "the secrets job wrote every secret ($generated values)" "[[ $generated -ge 10 ]]"
check "no secret is in any container's environment or command" "[[ $leaked == 0 ]]"
check "Colophon runs as uid 65532 on a read-only root" \
  "[[ \$(docker inspect -f '{{.Config.User}} {{.HostConfig.ReadonlyRootfs}}' \$(compose ps -q colophon)) == '65532:65532 true' ]]"

if [[ $browser == true ]]; then
  echo "==> the Studio journey in Chromium (playwright.quickstart.config.ts)"
  if (cd apps/studio && QUICKSTART_URL=$origin QUICKSTART_SETUP_CODE=$code \
    npx playwright test --config playwright.quickstart.config.ts); then
    pass "the Studio journey"
  else
    fail "the Studio journey"
  fi
fi

if [[ $load == true ]]; then
  echo "==> k6 smoke against the container (perf/k6/quickstart.js)"
  if BASE_URL=$origin k6 run --quiet --summary-export "$work/k6.json" perf/k6/quickstart.js; then
    pass "the k6 smoke's thresholds hold"
  else
    fail "the k6 smoke's thresholds hold"
  fi
  if [[ -f "$work/k6.json" ]]; then
    jq -r '"    \(.metrics.http_reqs.count) requests; p50 \(.metrics.http_req_duration.med | floor) ms, p99 \(.metrics.http_req_duration."p(99)" * 10 | floor / 10) ms"' "$work/k6.json"
  fi
fi

echo "==> the Colophon container alone: cold start to healthy"
compose stop colophon >/dev/null
t0=$(now)
compose up -d --wait --wait-timeout 120 colophon >/dev/null
cold_start=$(elapsed "$t0" "$(now)")
echo "    healthy in ${cold_start}s"

echo "==> down, then up again: the data and the secrets are kept"
compose down --timeout 20 >/dev/null
t0=$(now)
compose up -d --wait --wait-timeout 300
second_up=$(elapsed "$t0" "$(now)")
echo "    ready in ${second_up}s"
check "the secrets job found its secrets" "compose logs secrets | grep -q 'already generated'"
check "the server is ready with the same passwords" "compose exec -T colophon /usr/local/bin/colophon doctor --server http://127.0.0.1:8080"
if [[ $browser == true ]]; then
  check "the administrator the journey made is still there" \
    "curl -fsS '$origin/api/v1/installation' | jq -e '.data.has_administrator == true'"
fi

echo "==> sizes and memory"
size=$(docker image inspect -f '{{.Size}}' "$image")
pulled=$(docker save "$image" | gzip -c | wc -c | tr -d ' ')
running=()
while IFS= read -r id; do running+=("$id"); done < <(compose ps -q)
memory=$(docker stats --no-stream --format '{{.Name}} {{.MemUsage}}' "${running[@]}")
printf '    image %s: %s MB (docker image inspect), %s MB compressed (docker save | gzip)\n' "$image" $((size / 1000000)) $((pulled / 1000000))
while IFS= read -r line; do echo "    $line"; done <<<"$memory"

if [[ $record == true ]]; then
  cpu=$( (sysctl -n machdep.cpu.brand_string 2>/dev/null || grep -m1 'model name' /proc/cpuinfo | cut -d: -f2-) | sed 's/^ *//')
  cores=$(getconf _NPROCESSORS_ONLN)
  machine=$(printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//')
  stamp=$(date -u +%Y-%m-%dT%H%M%SZ)
  file="perf/results/$stamp-quickstart-$machine.json"
  colophon_memory=$(echo "$memory" | awk '/-colophon-/ {print $2}')
  jq -n --arg stamp "$stamp" --arg machine "$machine" --arg cpu "$cpu" --argjson cores "$cores" \
    --arg commit "$(git -C "$repo" rev-parse HEAD)" --arg image "$image" \
    --arg docker "$(docker version -f '{{.Server.Version}}')" --arg compose "$(docker compose version --short)" \
    --argjson first "$first_up" --argjson second "$second_up" --argjson cold "$cold_start" \
    --argjson size "$size" --argjson pulled "$pulled" --arg colophon_memory "$colophon_memory" \
    --argjson failures "$failures" \
    '{kind: "quickstart", stamp: $stamp, commit: $commit,
      machine: {id: $machine, cpu: $cpu, cores: $cores}, docker: $docker, compose: $compose,
      image: {ref: $image, bytes: $size, compressed_bytes: $pulled},
      seconds: {first_up_wait: $first, second_up_wait: $second, colophon_cold_start: $cold},
      colophon_idle_memory: $colophon_memory, failures: $failures,
      note: "images already present: pull time is not included"}' >"$repo/$file"
  echo "    recorded $file"
fi

if [[ $failures -gt 0 ]]; then
  echo "quickstart: $failures check(s) failed" >&2
  exit 1
fi
echo "quickstart: every check passed (first start ${first_up}s, restart ${second_up}s, cold start ${cold_start}s)"
