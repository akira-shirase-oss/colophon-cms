#!/usr/bin/env bash
# Installs Cloudflare's Authenticated Origin Pulls CA for Caddy (deploy/droplet/DEPLOY.md step 7),
# so the origin answers only connections that present Cloudflare's client certificate for this
# zone. Cloudflare's ranges are shared by every Cloudflare customer: without this, another account
# could point a proxied hostname at the droplet and reach it past this zone's rules.
#
# The certificate is in this directory (public; from
# https://developers.cloudflare.com/ssl/static/authenticated_origin_pull_ca.pem) and is installed
# only if its SHA-256 fingerprint is the one pinned here. --check compares it with Cloudflare's
# live copy too.
#
#   sudo deploy/droplet/origin-pull-ca.sh [--check]
set -euo pipefail
cd "$(dirname "$0")"
PINNED="9A:1A:C2:B4:BE:15:F9:F2:7E:EE:20:A7:34:CB:A4:E9:89:8F:61:00:1B:3B:D7:C8:4B:69:B5:6A:3E:25:A2:B9"
fingerprint() { openssl x509 -in "$1" -noout -fingerprint -sha256 | sed 's/^.*=//'; }

got=$(fingerprint cloudflare-origin-pull-ca.pem)
[[ "$got" == "$PINNED" ]] || { echo "error: cloudflare-origin-pull-ca.pem is $got, not the pinned $PINNED; nothing installed" >&2; exit 1; }
if [[ "${1:-}" == --check ]]; then
  live=$(mktemp); trap 'rm -f "$live"' EXIT
  curl -fsS --max-time 10 -o "$live" https://developers.cloudflare.com/ssl/static/authenticated_origin_pull_ca.pem
  [[ "$(fingerprint "$live")" == "$PINNED" ]] || { echo "error: Cloudflare's live origin-pull CA no longer matches the pin: update both together" >&2; exit 1; }
  echo "origin-pull CA: the pinned certificate is Cloudflare's current one (expires $(openssl x509 -in cloudflare-origin-pull-ca.pem -noout -enddate | sed 's/^notAfter=//'))"
  exit 0
fi
[[ $EUID -eq 0 ]] || { echo "error: run as root (or pass --check)" >&2; exit 2; }
install -m 0644 cloudflare-origin-pull-ca.pem /etc/caddy/cloudflare-origin-pull-ca.pem
echo "origin-pull CA installed: /etc/caddy/cloudflare-origin-pull-ca.pem ($PINNED)"
