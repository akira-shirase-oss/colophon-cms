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
domain="${SMOKE_DOMAIN:-app.colophon.404human.dev}"
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
