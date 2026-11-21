#!/usr/bin/env bash
# The droplet's Caddyfile, as install.sh renders it, run in the pinned Caddy image (Docker; on a
# Mac too). A stand-in upstream on :58000 echoes the X-Forwarded-For Caddy sends the server:
#   - no client certificate: the TLS handshake is refused (Authenticated Origin Pulls);
#   - from a trusted peer (Cloudflare), the server is told Cf-Connecting-Ip alone: a forged
#     X-Forwarded-For is replaced, not appended to (a Worker's or a visitor's left part is gone);
#   - from a peer that isn't trusted, the peer's own address, whatever the headers say;
#   - the admin API is a 0600 socket, not TCP 2019, and `caddy reload` (the unit's ExecReload)
#     works through it; no HTTP/3 is advertised;
#   - origin-pull-ca.sh's trust modes: the shared CA alone refuses our client certificate,
#     --add-custom and --custom-only take it (custom-origin-pull.sh generate made it).
#
#   deploy/droplet/test-caddy.sh
set -euo pipefail
cd "$(dirname "$0")"
here=$PWD
IMAGE="caddy:2.10.2@sha256:c3d7ee5d2b11f9dc54f947f68a734c84e9c9666c92c88a7f30b9cba5da182adb"
HOST=colophonapp.404human.dev
name="colophon-test-caddy-$$"
work=$(mktemp -d)
cleanup() { docker rm -f "$name" >/dev/null 2>&1 || true; rm -rf "$work"; }
trap cleanup EXIT
fail() {
  echo "FAIL: $*" >&2
  docker logs "$name" 2>&1 | tail -15 >&2 || true
  exit 1
}
ok() { echo "  ok    $1"; }

etc=$work/etc
mkdir -p "$etc"
# shellcheck source=deploy/droplet/public-host.sh
. ./public-host.sh
caddyfile_for_host Caddyfile "$HOST" >"$etc/Caddyfile"
# The stand-in server, for this test only: what Caddy forwarded, as text.
printf '\nhttp://:58000 {\n\trespond "xff=[{header.X-Forwarded-For}]"\n}\n' >>"$etc/Caddyfile"
openssl req -x509 -newkey rsa:2048 -nodes -subj "/CN=$HOST" -days 1 \
  -keyout "$etc/origin.key" -out "$etc/origin.pem" 2>/dev/null
trusted() { printf 'trusted_proxies static %s\n' "$1" >"$etc/cloudflare-trusted.caddy"; }
trusted '0.0.0.0/0 ::/0'

# Our own origin-pull client certificate, as the maintainer makes it.
./custom-origin-pull.sh generate "$work/aop" >"$work/generate.out" || fail "generate: $(cat "$work/generate.out")"
ORIGIN_PULL_CADDY_DIR=$etc ./origin-pull-ca.sh --shared >/dev/null
chmod -R a+rX "$etc"

docker run -d --name "$name" -p 127.0.0.1::443 -v "$etc:/etc/caddy" "$IMAGE" \
  sh -c 'mkdir -p /var/lib/caddy && exec caddy run --config /etc/caddy/Caddyfile --adapter caddyfile' >/dev/null
port=""
for _ in $(seq 40); do
  port=$(docker port "$name" 443/tcp 2>/dev/null | sed -n 's/^127\.0\.0\.1:\([0-9]*\)$/\1/p' | head -1)
  [[ -n "$port" ]] && docker exec "$name" test -S /var/lib/caddy/admin.sock 2>/dev/null && break
  sleep 0.25
done
[[ -n "$port" ]] || fail "Caddy didn't start"
reload() { docker exec "$name" caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile --force >/dev/null 2>&1; }
# get [curl args...]: the stand-in's answer through the site, with our client certificate.
get() {
  curl -sS --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" \
    --cert "$work/aop/client.pem" --key "$work/aop/client.key" "$@" "https://$HOST/livez" 2>&1 || true
}
# get_path <path>: the stand-in's answer for that path, redirects not followed.
get_path() {
  curl -sS --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" \
    --cert "$work/aop/client.pem" --key "$work/aop/client.key" "https://$HOST$1" 2>&1 || true
}

# The trust modes first: the shared CA alone can't verify our certificate.
[[ "$(get)" != xff=* ]] || fail "the shared CA alone accepted our own client certificate"
ok "trusting only Cloudflare's shared CA, our client certificate is refused"
ORIGIN_PULL_CADDY_DIR=$etc ./origin-pull-ca.sh --add-custom "$work/aop/ca.pem" >/dev/null
chmod -R a+rX "$etc"
reload || fail "reload after --add-custom"
[[ "$(get)" == xff=* ]] || fail "--add-custom: our client certificate was refused: $(get)"
grep -q cloudflare-origin-pull-ca.pem "$etc/origin-pull-trust.caddy" || fail "--add-custom dropped the shared CA (Cloudflare still presents it until the switch)"
ok "--add-custom: our client certificate is taken, and the shared CA still is"
ORIGIN_PULL_CADDY_DIR=$etc ./origin-pull-ca.sh --custom-only >/dev/null
reload || fail "reload after --custom-only"
[[ "$(get)" == xff=* ]] || fail "--custom-only: our client certificate was refused: $(get)"
if grep -q cloudflare-origin-pull-ca.pem "$etc/origin-pull-trust.caddy"; then fail "--custom-only still trusts the shared CA"; fi
ok "--custom-only: our CA alone is trusted"

# No client certificate at all.
code=$(curl -sS -o /dev/null -w '%{http_code}' --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" "https://$HOST/" 2>/dev/null || true)
[[ "$code" != 200 ]] || fail "served without a client certificate"
ok "no client certificate: refused"

# The client address the server is told.
got=$(get -H 'X-Forwarded-For: 6.6.6.6' -H 'Cf-Connecting-Ip: 203.0.113.7')
[[ "$got" == "xff=[203.0.113.7]" ]] || fail "from Cloudflare, with a forged X-Forwarded-For: the server was told '$got', not exactly the Cf-Connecting-Ip"
ok "from a trusted peer, the server is told Cf-Connecting-Ip alone; a forged X-Forwarded-For is replaced"
got=$(get -H 'X-Forwarded-For: 6.6.6.6')
[[ "$got" =~ ^xff=\[[0-9a-f.:]+\]$ && "$got" != *6.6.6.6* ]] || fail "from Cloudflare, no Cf-Connecting-Ip: the server was told '$got'"
ok "from a trusted peer without Cf-Connecting-Ip, the peer's address (${got#xff=}), not the forged header"
trusted '203.0.113.0/24'
reload || fail "reload with other trusted ranges"
got=$(get -H 'X-Forwarded-For: 6.6.6.6' -H 'Cf-Connecting-Ip: 1.2.3.4')
[[ "$got" =~ ^xff=\[[0-9a-f.:]+\]$ && "$got" != *6.6.6.6* && "$got" != *1.2.3.4* ]] || fail "from an untrusted peer: the server was told '$got'"
ok "from a peer that isn't trusted, its own address (${got#xff=}), whatever the headers say"

# The admin API.
mode=$(docker exec "$name" stat -c %a /var/lib/caddy/admin.sock)
[[ "$mode" == 600 ]] || fail "the admin socket is mode $mode, not 600"
if docker exec "$name" wget -qO- -T 2 http://127.0.0.1:2019/config/ >/dev/null 2>&1; then fail "the admin API answers on TCP 2019"; fi
reload || fail "caddy reload (the unit's ExecReload) failed through the socket"
ok "the admin API is a 0600 socket only; TCP 2019 is closed; caddy reload works through the socket"

# The bare root redirects to the Studio (302, so browsers don't keep it); nothing else does.
status_of() { # <path>: "<code> <location>"
  curl -sS -o /dev/null -w '%{http_code} %{redirect_url}' --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" \
    --cert "$work/aop/client.pem" --key "$work/aop/client.key" "https://$HOST$1" 2>/dev/null || true
}
[[ "$(status_of /)" == "302 https://$HOST/studio/" ]] || fail "GET / is '$(status_of /)', not a 302 to /studio/"
for p in /livez /studio/ /nope /studio; do
  got=$(get_path "$p")
  [[ "$got" == xff=* ]] || fail "GET $p didn't reach the server untouched: $got"
done
ok "GET / redirects 302 to /studio/; /livez, /studio/, /studio and an unknown /nope reach the server unredirected"

# HSTS for this host alone: a year, no includeSubDomains, no preload.
hsts=$(curl -sS -D - -o /dev/null --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" \
  --cert "$work/aop/client.pem" --key "$work/aop/client.key" "https://$HOST/" | tr -d '\r' | sed -n 's/^[Ss]trict-[Tt]ransport-[Ss]ecurity: //p')
[[ "$hsts" == "max-age=31536000" ]] || fail "Strict-Transport-Security is '$hsts', not exactly max-age=31536000"
ok "HSTS: max-age=31536000, this host only (no includeSubDomains, no preload)"

# HTTP/1.1 and HTTP/2 only.
if curl -sS -D - -o /dev/null --max-time 5 -k --connect-to "$HOST:443:127.0.0.1:$port" \
  --cert "$work/aop/client.pem" --key "$work/aop/client.key" "https://$HOST/" | grep -qi '^alt-svc:'; then
  fail "HTTP/3 is advertised (alt-svc)"
fi
ok "no HTTP/3 advertised"
cd "$here"
echo "caddy: the droplet's Caddyfile requires the origin-pull certificate, tells the server Cf-Connecting-Ip alone, and keeps its admin API on a 0600 socket"
