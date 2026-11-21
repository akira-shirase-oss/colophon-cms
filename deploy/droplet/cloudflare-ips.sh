#!/usr/bin/env bash
# Cloudflare's published address ranges, applied on the S0 droplet (deploy/droplet/DEPLOY.md):
#   - Caddy trusts them (/etc/caddy/cloudflare-trusted.caddy): from a peer in them, the client is
#     Cf-Connecting-Ip, and Caddy tells the server that one address (the Caddyfile);
#   - ufw lets only them reach 443 (SSH stays as you set it).
# The server trusts loopback alone (COLOPHON_TRUSTED_PROXIES=127.0.0.1/32 in colophon.env; an
# older run's Cloudflare list there is replaced): trusting Cloudflare's ranges there let a request
# from a Cloudflare address, such as another account's Worker, choose its own client address.
# Run as root after install and again whenever Cloudflare changes its list (rarely; a monthly
# timer is fine). Refuses to write anything from a list that doesn't look like address ranges.
#
#   sudo deploy/droplet/cloudflare-ips.sh [--print]
#
# For its test only (test-cloudflare-ips.sh): COLOPHON_CLOUDFLARE_IPS_TEST=1 skips the root check
# and takes the lists from $COLOPHON_CLOUDFLARE_IPS_FROM/ips-v4 and ips-v6, and the two files from
# COLOPHON_CADDY_TRUSTED_FILE and COLOPHON_ENV_FILE.
set -euo pipefail
testing="${COLOPHON_CLOUDFLARE_IPS_TEST:-0}"
if [[ "$testing" == 1 ]]; then
  v4=$(cat "$COLOPHON_CLOUDFLARE_IPS_FROM/ips-v4")
  v6=$(cat "$COLOPHON_CLOUDFLARE_IPS_FROM/ips-v6")
else
  v4=$(curl -fsS --max-time 10 https://www.cloudflare.com/ips-v4)
  v6=$(curl -fsS --max-time 10 https://www.cloudflare.com/ips-v6)
fi
ranges=$(printf '%s\n%s\n' "$v4" "$v6" | tr -d '\r' | sed '/^$/d')
count=$(wc -l <<<"$ranges" | tr -d ' ')
while IFS= read -r range; do
  [[ "$range" =~ ^[0-9]{1,3}(\.[0-9]{1,3}){3}/[0-9]{1,2}$ || "$range" =~ ^[0-9a-f:]+/[0-9]{1,3}$ ]] \
    || { echo "error: '$range' isn't an address range; nothing written" >&2; exit 1; }
done <<<"$ranges"
(( count >= 10 )) || { echo "error: only $count ranges from Cloudflare; nothing written" >&2; exit 1; }

if [[ "${1:-}" == --print ]]; then
  echo "$ranges"
  exit 0
fi
[[ $EUID -eq 0 || "$testing" == 1 ]] || { echo "error: run as root (or pass --print)" >&2; exit 2; }
caddy_file=/etc/caddy/cloudflare-trusted.caddy
env_file=/etc/colophon/colophon.env
if [[ "$testing" == 1 ]]; then caddy_file=$COLOPHON_CADDY_TRUSTED_FILE env_file=$COLOPHON_ENV_FILE; fi

joined=$(paste -sd' ' - <<<"$ranges")
printf 'trusted_proxies static %s\n' "$joined" >"$caddy_file"
# The server: loopback (Caddy) alone, the line replaced in place (one line, whatever was there).
tmp=$(mktemp)
grep -v '^COLOPHON_TRUSTED_PROXIES=' "$env_file" >"$tmp" || true
echo "COLOPHON_TRUSTED_PROXIES=127.0.0.1/32" >>"$tmp"
cat "$tmp" >"$env_file" && rm -f "$tmp"

# ufw: 443 from Cloudflare only. Existing rules for 443 are replaced; SSH is left alone.
# No pipeline here has a reader that stops early (grep -q, head): under pipefail its writer's
# SIGPIPE (141) fails the pipeline, which read as "no rule left" in the loop's test, or killed the
# script, as `yes | ufw delete` did on every re-run (2026-11-21). ufw's answers are read whole
# first; --force replaces ufw's y/n question. The port boundary avoids \b, which BSD grep lacks,
# so this runs the same under the test on a Mac.
rule443='(^|[^0-9])443(/tcp)?([^0-9]|$)'
rules=$(ufw status numbered)
while grep -qE "$rule443" <<<"$rules"; do
  n=$(grep -m 1 -E "$rule443" <<<"$rules" | sed -E 's/^\[ *([0-9]+)\].*/\1/')
  ufw --force delete "$n" >/dev/null
  rules=$(ufw status numbered)
done
while IFS= read -r range; do
  ufw allow proto tcp from "$range" to any port 443 comment 'cloudflare' >/dev/null
done <<<"$ranges"

echo "cloudflare-ips: $count ranges trusted by Caddy and allowed to 443; Colophon trusts loopback alone"
echo "next: sudo systemctl reload caddy && sudo systemctl restart colophon"
