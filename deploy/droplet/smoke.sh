#!/usr/bin/env bash
# After install, and after every upgrade: is the S0 droplet serving, safely, through Cloudflare?
# Run on the droplet as root (it reads the unit's state and, optionally, a token file).
#
#   sudo deploy/droplet/smoke.sh                       # health, TLS, headers, delivery, memory
#   sudo SMOKE_SCOPE=local deploy/droplet/smoke.sh     # the server itself only, on loopback (what
#                                                      # upgrade.sh gates a switch on: a Cloudflare
#                                                      # hiccup mustn't roll back a good release)
#   sudo SMOKE_PROJECT=<project id> SMOKE_TOKEN_FILE=/root/smoke-token deploy/droplet/smoke.sh
#                                                      # also: an upload lands in R2 and verifies
#
# SMOKE_TOKEN_FILE holds a project API token (full-access) made in the Studio for this check; keep
# it root-only (chmod 600) and revoke it when S0 is signed off. Exits non-zero on any failure, after
# printing every check. That the droplet's own address refuses 443 (only Cloudflare may reach it)
# is checked from another machine: DEPLOY.md step 11.
# shellcheck disable=SC2015  # ok and bad only print, so `test && ok || bad` is if-then-else here.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=deploy/droplet/public-host.sh
. "$here/public-host.sh"
# SMOKE_DOMAIN, else the host of COLOPHON_PUBLIC_URL in the droplet's env file, else the shipped name.
domain="${SMOKE_DOMAIN:-}"
[[ -n "$domain" ]] || domain=$(droplet_domain /etc/colophon/colophon.env) || { echo "smoke: no domain to check" >&2; exit 1; }
scope="${SMOKE_SCOPE:-full}"
local_url="http://127.0.0.1:58000"
public="https://$domain"
failures=0
ok() { printf '  ok    %s\n' "$1"; }
bad() { printf '  FAIL  %s\n' "$1"; failures=$((failures + 1)); }
check() { local what=$1; shift; if "$@" >/dev/null 2>&1; then ok "$what"; else bad "$what"; fi; }

echo "services"
for unit in colophon caddy valkey-server redis-server; do
  systemctl list-unit-files "$unit.service" >/dev/null 2>&1 || continue
  systemctl cat "$unit.service" >/dev/null 2>&1 || continue
  check "$unit is active" systemctl is-active --quiet "$unit"
done

echo "the server, on loopback"
check "/livez answers 200" curl -fsS --max-time 5 "$local_url/livez"
ready=$(curl -sS --max-time 5 "$local_url/readyz" || true)
if jq -e '.status == "ready"' <<<"$ready" >/dev/null 2>&1; then ok "/readyz is ready ($(jq -c '.checks' <<<"$ready"))"; else bad "/readyz is not ready: $ready"; fi
version=$(curl -fsS --max-time 5 "$local_url/version" || true)
jq -e '.version' <<<"$version" >/dev/null 2>&1 && ok "/version: $(jq -r '.version + " " + (.commit // "")' <<<"$version")" || bad "/version: $version"

# The runtime's role must not bypass row-level security: Neon's owner role has BYPASSRLS, so an
# app URL that names it would quietly turn tenant isolation off (init-roles' colophon_app has none).
creds="${SMOKE_CREDENTIALS:-/etc/colophon/credentials}"
if command -v psql >/dev/null && [[ -r "$creds/database_url" ]]; then
  role=$(PGCONNECT_TIMEOUT=10 psql "$(cat "$creds/database_url")" -XAtqc "select current_user || ' ' || rolbypassrls || ' ' || rolsuper from pg_roles where rolname = current_user" 2>/dev/null)
  case "$role" in
    *" false false") ok "the server's database role (${role%% *}) has neither BYPASSRLS nor superuser" ;;
    "") bad "couldn't read the server's database role" ;;
    *) bad "the server's database role ${role%% *} bypasses row-level security or is a superuser: use init-roles' colophon_app" ;;
  esac
fi

# The database round trip from here (Toronto ↔ Neon's us-east-2): 20 × SELECT 1 in one session
# over the server's own verify-full URL, so the TLS handshake is paid once. Reported, warned on
# above 20 ms at p50; readiness above decides pass or fail.
creds="${SMOKE_CREDENTIALS:-/etc/colophon/credentials}"
if command -v psql >/dev/null && [[ -r "$creds/database_url" ]]; then
  queries=(-c '\timing on'); for _ in $(seq 20); do queries+=(-c 'SELECT 1'); done
  times=$(PGCONNECT_TIMEOUT=10 psql "$(cat "$creds/database_url")" -XAtq "${queries[@]}" 2>/dev/null \
    | sed -n 's/^Time: \([0-9.]*\) ms.*/\1/p' | sort -n)
  if [[ $(wc -l <<<"$times" | tr -d ' ') -ge 10 ]]; then
    p50=$(awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}' <<<"$times")
    p95=$(awk '{a[NR]=$1} END {i=int(NR*0.95+0.5); if (i<1) i=1; print a[i]}' <<<"$times")
    if awk -v p="$p50" 'BEGIN {exit !(p > 20)}'; then
      printf '  warn  database round trip p50 %s ms, p95 %s ms (over 20 ms at p50: check the region and the direct endpoint)\n' "$p50" "$p95"
    else
      ok "database round trip p50 $p50 ms, p95 $p95 ms ($(wc -l <<<"$times" | tr -d ' ') × SELECT 1)"
    fi
  else
    printf '  warn  database round trip not measured (psql got no timings)\n'
  fi
fi

# A non-superuser migration's grants (D-234) exist only inside its transaction: none may be left.
# migrate-status reads the owner URL from its file (never a command line) and reports them.
if command -v colophon >/dev/null && [[ -r "$creds/migration_database_url" ]]; then
  status=$(colophon admin migrate-status --database-url-file "$creds/migration_database_url" --json 2>/dev/null || true)
  grants=$(jq -c '.data.grants // empty' <<<"$status" 2>/dev/null || true)
  if [[ "$grants" == '{"create_on_public":[],"deleter_inherited":false}' ]]; then
    ok "no migration grant left behind (CREATE on public, the deleter's INHERIT)"
  elif [[ -z "$grants" ]]; then
    bad "couldn't read the migration grants from colophon admin migrate-status"
  else
    bad "migration grants left behind: $grants (colophon admin migrate removes them; D-234)"
  fi
fi

echo "the Studio and delivery, on loopback"
headers=$(curl -sSI --max-time 5 "$local_url/studio/" || true)
grep -qiE '^HTTP/[0-9.]+ 200' <<<"$headers" && ok "the Studio answers 200" || bad "the Studio: $(head -1 <<<"$headers")"
grep -qi '^content-security-policy:' <<<"$headers" && ok "the Studio has a content security policy" || bad "no content-security-policy on /studio/"
body=$(curl -sS --max-time 5 "$local_url/content/v1/00000000-0000-4000-8000-000000000000/production/articles" || true)
jq -e '.error.code == "not_found"' <<<"$body" >/dev/null 2>&1 && ok "anonymous delivery of nothing is a 404 envelope" || bad "anonymous delivery: $body"
if [[ -n "${SMOKE_EXPECT_VERSION:-}" ]]; then
  [[ "$(jq -r '.version' <<<"$version" 2>/dev/null)" == "$SMOKE_EXPECT_VERSION" ]] && ok "the server is v$SMOKE_EXPECT_VERSION" || bad "the server is $(jq -r '.version' <<<"$version" 2>/dev/null), not $SMOKE_EXPECT_VERSION"
fi

if [[ "$scope" == local ]]; then
  echo
  if (( failures > 0 )); then echo "smoke (local): $failures check(s) FAILED"; exit 1; fi
  echo "smoke (local): every check passed"
  exit 0
fi

echo "through Cloudflare"
# The server keys every per-address limit — the 5-a-second password limit above all (D-213) — by
# the visitor X-Forwarded-For names past the proxies it trusts. Trusting only loopback, it would
# see every visitor as a Cloudflare edge, and one visitor's flood would shut everyone out.
proxies=$(sed -n 's/^COLOPHON_TRUSTED_PROXIES=//p' "${SMOKE_ENV_FILE:-/etc/colophon/colophon.env}" | tail -1)
ranges=$(tr ',' '\n' <<<"$proxies" | grep -cv '^127\.0\.0\.1\(/32\)\?$\|^$' || true)
if (( ranges >= 10 )); then
  ok "the server trusts loopback and $ranges Cloudflare ranges, so limits key by the visitor"
else
  bad "COLOPHON_TRUSTED_PROXIES has $ranges Cloudflare ranges besides loopback: every visitor would share one per-address limit (run cloudflare-ips.sh)"
fi
code=$(curl -sS -o /dev/null -w '%{http_code} %{ssl_verify_result}' --max-time 10 "$public/livez" || echo "000 x")
[[ "$code" == "200 0" ]] && ok "$public/livez answers 200 over valid TLS" || bad "$public/livez: HTTP/verify $code"
# Authenticated Origin Pulls: straight to the origin, without Cloudflare's client certificate, the
# TLS handshake is refused (Caddy requires the certificate; only Cloudflare holds it).
direct=$(curl -sS -o /dev/null -w '%{http_code}' --max-time 5 -k --resolve "$domain:443:127.0.0.1" "$public/livez" 2>/dev/null || true)
[[ "$direct" != 200 ]] && ok "the origin refuses a connection without Cloudflare's client certificate" || bad "the origin served a connection without Cloudflare's client certificate"
headers=$(curl -sSI --max-time 10 "$public/studio/" || true)
grep -qi '^server: cloudflare' <<<"$headers" && ok "served through Cloudflare" || bad "not through Cloudflare (no server: cloudflare header)"
grep -qi '^content-security-policy:' <<<"$headers" && ok "the Studio has a content security policy" || bad "no content-security-policy on /studio/"
grep -qiE '^HTTP/[0-9.]+ 200' <<<"$headers" && ok "the Studio answers 200" || bad "the Studio: $(head -1 <<<"$headers")"
body=$(curl -sS --max-time 10 "$public/content/v1/00000000-0000-4000-8000-000000000000/production/articles" || true)
jq -e '.error.code == "not_found"' <<<"$body" >/dev/null 2>&1 && ok "anonymous delivery of nothing is a 404 envelope" || bad "anonymous delivery: $body"

echo "backups"
if command -v colophon-backup-logical >/dev/null; then
  latest=$(colophon-backup-logical latest 2>/dev/null || echo none)
  hours=${latest##* }
  if [[ "$latest" != none && "$hours" =~ ^[0-9]+$ ]] && (( hours < 26 )); then ok "the newest logical backup is ${hours} h old (${latest%% *})"
  else bad "no logical backup in the last 26 h ($latest)"; fi
else
  bad "colophon-backup-logical isn't installed (DEPLOY.md step 10)"
fi
check "the ledger export timer is active" systemctl is-active --quiet colophon-ledger-export.timer
check "the logical backup timer is active" systemctl is-active --quiet colophon-backup-logical.timer

echo "memory (S0 exit gate: recorded idle)"
current=$(systemctl show colophon -p MemoryCurrent --value 2>/dev/null || echo "")
if [[ "$current" =~ ^[0-9]+$ ]]; then ok "colophon uses $((current / 1024 / 1024)) MiB"; else bad "no MemoryCurrent for colophon ($current)"; fi
free -m | awk '/^Mem:/ {printf "  info  host: %d MiB used of %d, %d available\n", $3, $2, $7}'

if [[ -n "${SMOKE_TOKEN_FILE:-}" && -n "${SMOKE_PROJECT:-}" ]]; then
  echo "an upload, end to end (R2)"
  token=$(tr -d '\n' <"$SMOKE_TOKEN_FILE")
  png=$(mktemp); trap 'rm -f "$png"' EXIT
  # A 1x1 transparent PNG.
  base64 -d >"$png" <<<'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=='
  env_path="$public/api/v1/projects/$SMOKE_PROJECT/environments/production"
  up=$(curl -sS --max-time 30 -X POST "$env_path/assets" -H "authorization: Bearer $token" \
    -H 'content-type: image/png' -H 'x-colophon-file-name: smoke.png' -H "idempotency-key: smoke-$(date +%s)" \
    --data-binary @"$png" || true)
  asset=$(jq -r '.data.id // empty' <<<"$up" 2>/dev/null)
  if [[ -z "$asset" ]]; then
    bad "upload refused: $up"
  else
    ok "uploaded asset $asset"
    verified=""
    for _ in $(seq 30); do
      got=$(curl -sS --max-time 10 "$env_path/assets/$asset" -H "authorization: Bearer $token" || true)
      if jq -e '(.data.versions // []) | length > 0' <<<"$got" >/dev/null 2>&1; then verified=$got; break; fi
      sleep 2
    done
    [[ -n "$verified" ]] && ok "the worker verified it in R2" || bad "not verified within 60 s: $got"
  fi
fi

echo
if (( failures > 0 )); then echo "smoke: $failures check(s) FAILED"; exit 1; fi
echo "smoke: every check passed"
