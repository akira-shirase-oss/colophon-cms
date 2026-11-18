#!/usr/bin/env bash
# Tests the shared-stack guard (D-187): on a stack this run didn't make (no CI, no
# COMPOSE_PROJECT_NAME of its own, no COLOPHON_STACK_OWNER=1), nothing stops, pauses or
# reconfigures a service, and nothing writes to the everyday database:
#
#   - scripts/stack.sh check|down|reset refuse, before calling docker at all (up is allowed: it is
#     idempotent and destroys nothing);
#   - scripts/load.sh refuses the scenarios that pause Postgres or Valkey (cache-down,
#     postgres-trouble), before calling docker;
#   - scripts/migrate.sh (which load.sh, testbed.sh and studio.sh call) refuses without a
#     COLOPHON_TEST_DATABASE of the run's own, before building anything;
#   - scripts/check.sh's integration half, on a shared stack, plans no `stack.sh check`, skips the
#     four tests that stop services by name, leaves out the two pausing smokes, needs its own test
#     database and never drops colophon_check; on its own stack it plans everything.
#
# docker, cargo and k6 are stand-ins that only log, so the test never touches a real stack.
#
#   scripts/test-stack-guard.sh
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() {
  echo "FAIL: $*" >&2
  exit 1
}

mkdir "$work/bin"
for tool in docker cargo k6; do
  printf '#!/usr/bin/env bash\necho "%s $*" >>"%s/calls"\n' "$tool" "$work" >"$work/bin/$tool"
  chmod +x "$work/bin/$tool"
done
: >"$work/calls"

# A shell with none of the "this stack is ours" signals.
shared() { env -u CI -u COMPOSE_PROJECT_NAME -u COLOPHON_STACK_OWNER -u COLOPHON_TEST_DATABASE PATH="$work/bin:$PATH" "$@"; }

# expect_refused WHAT COMMAND...: exit 2, a message naming the guard, and no docker or cargo call.
expect_refused() {
  local what=$1 status=0 out
  shift
  : >"$work/calls"
  out=$("$@" 2>&1) || status=$?
  [[ "$status" == 2 ]] || fail "$what: exit $status, wanted 2 ($out)"
  grep -q "D-187" <<<"$out" || fail "$what: the refusal doesn't name the rule: $out"
  ! grep -qE '^(docker|cargo) ' "$work/calls" || fail "$what: called $(cat "$work/calls") before refusing"
}

# The maintainer's real situation: the project is `colophon`, no CI, no owner flag.
for sub in check down reset; do
  expect_refused "stack.sh $sub" shared scripts/stack.sh "$sub"
  expect_refused "stack.sh $sub, project named colophon" shared env COMPOSE_PROJECT_NAME=colophon scripts/stack.sh "$sub"
done
expect_refused "load.sh cache-down" shared scripts/load.sh cache-down throughput
expect_refused "load.sh postgres-trouble" shared scripts/load.sh postgres-trouble throughput
expect_refused "migrate.sh without a test database" shared scripts/migrate.sh
expect_refused "migrate.sh naming the everyday database" shared env COLOPHON_TEST_DATABASE=colophon scripts/migrate.sh

# stack.sh up is allowed on a shared stack: it reaches docker.
: >"$work/calls"
shared scripts/stack.sh up >/dev/null 2>&1 || true
grep -q '^docker compose .*up' "$work/calls" || fail "stack.sh up was refused on a shared stack: $(cat "$work/calls")"

# CI, the owner, and a stack of one's own may: stack.sh down reaches docker.
: >"$work/calls"
shared env CI=true scripts/stack.sh down >/dev/null 2>&1 || true
grep -q '^docker compose .*down' "$work/calls" || fail "stack.sh down in CI didn't reach docker: $(cat "$work/calls")"
: >"$work/calls"
shared env CI=true scripts/load.sh cache-down throughput >/dev/null 2>&1 || true
! grep -q "D-187" <<<"$(shared env CI=true scripts/load.sh cache-down throughput 2>&1 || true)" || fail "load.sh cache-down was refused in CI"
: >"$work/calls"
shared env COLOPHON_STACK_OWNER=1 scripts/stack.sh down >/dev/null 2>&1 || true
grep -q '^docker compose .*down' "$work/calls" || fail "the owner's stack.sh down didn't reach docker: $(cat "$work/calls")"
: >"$work/calls"
shared env COMPOSE_PROJECT_NAME=colophon-mine scripts/stack.sh down >/dev/null 2>&1 || true
grep -q '^docker compose .*down' "$work/calls" || fail "stack.sh down on an own project didn't reach docker: $(cat "$work/calls")"

# check.sh's integration plan, printed without running anything.
plan_shared=$(shared env COLOPHON_TEST_DATABASE=colophon_mine scripts/check.sh --integration-plan)
! grep -qx 'scripts/stack.sh check' <<<"$plan_shared" || fail "shared plan runs stack.sh check: $plan_shared"
for test in admission::without_valkey_each_server_still_limits_on_its_own \
  quotas::without_valkey_a_token_is_still_limited_never_unlimited \
  cache::answers_survive_valkey_going_away_and_caching_resumes_when_it_is_back \
  lifecycle::readiness_follows_real_outages_and_recoveries; do
  grep -q -- "--skip $test" <<<"$plan_shared" || fail "shared plan doesn't skip $test: $plan_shared"
done
# libtest names an integration test without its file (`without_valkey_…`, not
# `admission::without_valkey_…`): a skip with the file prefix matches nothing, and the test runs.
for name in without_valkey_each_server_still_limits_on_its_own without_valkey_a_token_is_still_limited_never_unlimited \
  answers_survive_valkey_going_away_and_caching_resumes_when_it_is_back readiness_follows_real_outages_and_recoveries; do
  grep -qE -- "--skip $name( |\$)" <<<"$plan_shared" || fail "the shared plan doesn't skip $name by the name libtest gives it: $plan_shared"
done
! grep -qE -- '--skip [a-z_]+::' <<<"$plan_shared" || fail "the shared plan skips by file::name, which libtest never matches: $plan_shared"
! grep -qE 'load.sh (cache-down|postgres-trouble) ' <<<"$plan_shared" || fail "shared plan runs a pausing smoke: $plan_shared"
! grep -q 'DROP DATABASE' <<<"$plan_shared" || fail "shared plan drops a database: $plan_shared"
grep -q 'load.sh tokens throughput' <<<"$plan_shared" || fail "shared plan lost an ordinary smoke: $plan_shared"
grep -q 'CI only' <<<"$plan_shared" || fail "shared plan doesn't say what it leaves to CI: $plan_shared"

status=0
out=$(shared scripts/check.sh --integration-plan 2>&1) || status=$?
[[ "$status" == 2 ]] || fail "a shared plan without COLOPHON_TEST_DATABASE: exit $status, wanted 2 ($out)"

plan_own=$(shared env COLOPHON_STACK_OWNER=1 scripts/check.sh --integration-plan)
grep -qx 'scripts/stack.sh check' <<<"$plan_own" || fail "the owner's plan lacks stack.sh check: $plan_own"
grep -q 'load.sh cache-down ' <<<"$plan_own" || fail "the owner's plan lacks the cache-down smoke: $plan_own"
! grep -q -- '--skip ' <<<"$plan_own" || fail "the owner's plan skips tests: $plan_own"

# The testbed's outage spec stops Postgres and Valkey: it asks the same question in TypeScript
# (apps/testbed/tests/stack-owner.ts) and must answer as stack_is_ours does, case by case.
[[ -f apps/testbed/tests/stack-owner.ts ]] || fail "the testbed's outage spec has no stack-owner rule (apps/testbed/tests/stack-owner.ts)"
grep -q "stackIsOurs" apps/testbed/tests/outage.spec.ts || fail "outage.spec.ts doesn't ask stackIsOurs before stopping services"
verdicts() {
  local ts sh
  ts=$(env -u CI -u COMPOSE_PROJECT_NAME -u COLOPHON_STACK_OWNER "$@" node --input-type=module \
    -e "const { stackIsOurs } = await import('./apps/testbed/tests/stack-owner.ts'); console.log(stackIsOurs() ? 'ours' : 'shared')" 2>/dev/null)
  sh=$(env -u CI -u COMPOSE_PROJECT_NAME -u COLOPHON_STACK_OWNER "$@" bash -c '. scripts/stack-guard.sh; stack_is_ours && echo ours || echo shared')
  [[ "$ts" == "$sh" ]] || fail "for '$*' the testbed says '$ts', the guard says '$sh'"
}
verdicts true
verdicts CI=true
verdicts COMPOSE_PROJECT_NAME=colophon
verdicts COMPOSE_PROJECT_NAME=colophon-ci
verdicts COLOPHON_STACK_OWNER=1
verdicts COLOPHON_STACK_OWNER=0

echo "stack guard: every refusal and plan as specified"
