#!/usr/bin/env bash
# Which client certificates Caddy accepts on 443: Authenticated Origin Pulls (DEPLOY.md step 7).
#
# Cloudflare's DEFAULT origin-pull certificate is one certificate SHARED by every Cloudflare
# customer: trusting its CA keeps out everything that isn't Cloudflare, but not another Cloudflare
# account that points a hostname here. Our own certificate, uploaded to this zone
# (custom-origin-pull.sh), is presented by this zone only. So:
#
#   sudo origin-pull-ca.sh                    install the shared CA; keep the current mode (the
#                                             shared CA alone on a first run)
#   sudo origin-pull-ca.sh --shared           trust the shared CA alone
#   sudo origin-pull-ca.sh --add-custom <ca>  trust our CA as well as the shared one: before our
#                                             certificate is enabled at Cloudflare
#   sudo origin-pull-ca.sh --custom-only      trust our CA alone: once Cloudflare presents ours
#   origin-pull-ca.sh --check                 the pinned shared CA is Cloudflare's current one
#
# Caddy's trust_pool reads one certificate per file, so each CA is its own file, and the Caddyfile
# imports /etc/caddy/origin-pull-trust.caddy, which this script writes. The shared CA (public; from
# https://developers.cloudflare.com/ssl/static/authenticated_origin_pull_ca.pem) is installed only
# if its SHA-256 fingerprint is the one pinned here. Reload Caddy afterwards (install.sh does).
set -euo pipefail
cd "$(dirname "$0")"
PINNED="9A:1A:C2:B4:BE:15:F9:F2:7E:EE:20:A7:34:CB:A4:E9:89:8F:61:00:1B:3B:D7:C8:4B:69:B5:6A:3E:25:A2:B9"
# Where the files go, and the path Caddy reads them at (the same on the droplet; a test points the
# first elsewhere and mounts it at the second).
ETC=${ORIGIN_PULL_CADDY_DIR:-/etc/caddy}
CADDY=/etc/caddy
SHARED=cloudflare-origin-pull-ca.pem CUSTOM=origin-pull-custom-ca.pem TRUST=origin-pull-trust.caddy
fingerprint() { openssl x509 -in "$1" -noout -fingerprint -sha256 | sed 's/^.*=//'; }
die() { echo "error: $*" >&2; exit 1; }

got=$(fingerprint cloudflare-origin-pull-ca.pem)
[[ "$got" == "$PINNED" ]] || die "cloudflare-origin-pull-ca.pem is $got, not the pinned $PINNED; nothing installed"
mode=${1:-}
if [[ "$mode" == --check ]]; then
  live=$(mktemp); trap 'rm -f "$live"' EXIT
  curl -fsS --max-time 10 -o "$live" https://developers.cloudflare.com/ssl/static/authenticated_origin_pull_ca.pem
  [[ "$(fingerprint "$live")" == "$PINNED" ]] || die "Cloudflare's live origin-pull CA no longer matches the pin: update both together"
  echo "origin-pull CA: the pinned certificate is Cloudflare's current one (expires $(openssl x509 -in cloudflare-origin-pull-ca.pem -noout -enddate | sed 's/^notAfter=//'))"
  exit 0
fi
[[ $EUID -eq 0 || -n "${ORIGIN_PULL_CADDY_DIR:-}" ]] || die "run as root (or pass --check)"

# trust <file>...: the snippet Caddy imports inside client_auth, written whole then moved.
trust() {
  local f tmp
  tmp=$(mktemp "$ETC/.trust.XXXXXX")
  printf 'trust_pool file' >"$tmp"
  for f in "$@"; do printf ' %s/%s' "$CADDY" "$f" >>"$tmp"; done
  printf '\n' >>"$tmp"
  chmod 0644 "$tmp" && mv -f "$tmp" "$ETC/$TRUST"
}

install -m 0644 cloudflare-origin-pull-ca.pem "$ETC/$SHARED"
case "$mode" in
  "")
    if [[ -s "$ETC/$TRUST" ]]; then
      if grep -q "$CUSTOM" "$ETC/$TRUST" && [[ ! -s "$ETC/$CUSTOM" ]]; then die "$TRUST names our CA, but $ETC/$CUSTOM is missing: run --add-custom <ca.pem> again"; fi
      echo "origin-pull CA: the shared CA installed ($PINNED); trust kept as it was: $(cat "$ETC/$TRUST")"
    else
      trust "$SHARED"
      echo "origin-pull CA: the shared CA installed and trusted ($PINNED)"
    fi ;;
  --shared)
    trust "$SHARED"
    echo "origin-pull CA: the shared CA alone is trusted" ;;
  --add-custom)
    ca=${2:-}
    [[ -s "$ca" ]] || die "usage: origin-pull-ca.sh --add-custom <ca.pem> (custom-origin-pull.sh generate made it)"
    openssl x509 -in "$ca" -noout -ext basicConstraints 2>/dev/null | grep -q 'CA:TRUE' || die "$ca isn't a CA certificate"
    openssl x509 -in "$ca" -noout -checkend 0 >/dev/null || die "$ca has expired"
    [[ "$(fingerprint "$ca")" != "$PINNED" ]] || die "$ca is Cloudflare's shared CA, not ours"
    install -m 0644 "$ca" "$ETC/$CUSTOM"
    trust "$SHARED" "$CUSTOM"
    echo "origin-pull CA: ours ($(fingerprint "$ETC/$CUSTOM")) trusted beside the shared one"
    echo "next: upload and enable our certificate at Cloudflare (custom-origin-pull.sh), then --custom-only" ;;
  --custom-only)
    [[ -s "$ETC/$CUSTOM" ]] || die "no CA of ours at $ETC/$CUSTOM: run --add-custom <ca.pem> first"
    trust "$CUSTOM"
    echo "origin-pull CA: ours alone is trusted ($(fingerprint "$ETC/$CUSTOM")); the shared certificate is refused from now on" ;;
  *) echo "usage: origin-pull-ca.sh [--shared | --add-custom <ca.pem> | --custom-only | --check]" >&2; exit 2 ;;
esac
