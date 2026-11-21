#!/usr/bin/env bash
# Zone-level Authenticated Origin Pulls with OUR OWN client certificate (DEPLOY.md, "Our own
# origin-pull certificate"). Cloudflare's default origin-pull certificate is one certificate shared
# by every Cloudflare customer: an origin that trusts it accepts any Cloudflare zone's requests, not
# only this zone's. With a certificate of our own uploaded to this zone, only this zone presents it.
#
# Run on the maintainer's machine, not the droplet (only the CA certificate goes there):
#
#   custom-origin-pull.sh generate [<new dir>]    a client CA and a client certificate it signed,
#                                                 in a new 0700 directory (default: mktemp -d)
#   custom-origin-pull.sh print <zone-id> <dir>   the Cloudflare API calls, printed, not run
#   custom-origin-pull.sh upload <zone-id> <dir>  uploads the client certificate and its key to the
#                                                 zone; needs CLOUDFLARE_API_TOKEN (Zone: SSL and
#                                                 Certificates: Edit) in the environment
#   custom-origin-pull.sh status <zone-id>        the zone's uploaded certificates and their status
#   custom-origin-pull.sh enable <zone-id>        turns zone-level AOP on (it is on already for S0;
#                                                 Cloudflare then presents our certificate)
#
# The order, so the origin never refuses Cloudflare (DEPLOY.md has it in full):
#   1. generate; copy <dir>/ca.pem to the droplet; there: origin-pull-ca.sh --add-custom ca.pem
#      (Caddy trusts the shared CA and ours);
#   2. upload; status until "active"; enable;
#   3. on the droplet: origin-pull-ca.sh --custom-only (ours alone), then smoke.sh.
# The token is sent from a 0600 file (curl -H @file), and the key from a file (--data-binary @),
# never on a command line. Nothing secret is printed.
set -euo pipefail
API=${CLOUDFLARE_API:-https://api.cloudflare.com/client/v4}
die() { echo "error: $*" >&2; exit 1; }
zone_ok() { [[ "${1:-}" =~ ^[0-9a-f]{32}$ ]] || die "a zone id is 32 hex characters (the zone's Overview page, API section)"; }
dir_ok() { [[ -s "$1/client.pem" && -s "$1/client.key" ]] || die "$1 has no client.pem and client.key (run generate first)"; }

generate() {
  local dir=${1:-}
  if [[ -z "$dir" ]]; then dir=$(mktemp -d); else
    [[ ! -e "$dir" ]] || die "$dir exists; give a new directory"
    mkdir -m 0700 "$dir"
  fi
  chmod 0700 "$dir"
  (
    umask 077
    cd "$dir"
    openssl req -x509 -new -nodes -newkey ec -pkeyopt ec_paramgen_curve:P-256 -days 3650 \
      -subj "/CN=Colophon origin-pull CA" \
      -addext "basicConstraints=critical,CA:TRUE,pathlen:0" -addext "keyUsage=critical,keyCertSign,cRLSign" \
      -keyout ca.key -out ca.pem 2>/dev/null
    openssl req -new -nodes -newkey ec -pkeyopt ec_paramgen_curve:P-256 -subj "/CN=Cloudflare origin pull" \
      -keyout client.key -out client.csr 2>/dev/null
    printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n' >client.ext
    openssl x509 -req -days 1825 -in client.csr -CA ca.pem -CAkey ca.key -CAcreateserial -extfile client.ext \
      -out client.pem 2>/dev/null
    rm -f client.csr client.ext ca.srl
    openssl verify -CAfile ca.pem -purpose sslclient client.pem >/dev/null || { echo "error: the client certificate doesn't verify against the CA" >&2; exit 1; }
    chmod 0644 ca.pem client.pem
  )
  cat <<EOF
generated in $dir (0700):
  ca.pem       the CA certificate: copy it to the droplet (public)
  client.pem   the client certificate, and
  client.key   its key: both go to Cloudflare (upload), nowhere else
  ca.key       the CA's key: only for issuing the next client certificate; keep it offline, or delete it
CA         $(openssl x509 -in "$dir/ca.pem" -noout -fingerprint -sha256 | sed 's/^.*=//')
client     expires $(openssl x509 -in "$dir/client.pem" -noout -enddate | sed 's/^notAfter=//')
next: scp $dir/ca.pem root@<droplet>:/root/origin-pull-ca.pem; on the droplet: origin-pull-ca.sh --add-custom /root/origin-pull-ca.pem
EOF
}

# body <dir>: the upload's JSON, into a 0600 file; prints its path.
body() {
  local f
  f=$(mktemp); chmod 0600 "$f"
  jq -n --rawfile certificate "$1/client.pem" --rawfile private_key "$1/client.key" \
    '{certificate: $certificate, private_key: $private_key}' >"$f"
  printf '%s' "$f"
}

# api <method> <path> [curl args...]: one call, the token from a 0600 header file.
api() {
  local method=$1 path=$2 headers out; shift 2
  [[ -n "${CLOUDFLARE_API_TOKEN:-}" ]] || die "CLOUDFLARE_API_TOKEN isn't set (a token with Zone: SSL and Certificates: Edit, for this zone only)"
  headers=$(mktemp); chmod 0600 "$headers"
  printf 'Authorization: Bearer %s\nContent-Type: application/json\n' "$CLOUDFLARE_API_TOKEN" >"$headers"
  out=$(curl -sS --max-time 30 -X "$method" -H @"$headers" "$@" "$API$path") || { rm -f "$headers"; die "$method $path: no answer"; }
  rm -f "$headers"
  jq -e '.success == true' <<<"$out" >/dev/null 2>&1 || die "$method $path: $(jq -c '.errors // .' <<<"$out" 2>/dev/null || echo "$out" | head -c 300)"
  printf '%s' "$out"
}

print_calls() {
  local zone=$1 dir=$2
  cat <<EOF
# With CLOUDFLARE_API_TOKEN set (Zone: SSL and Certificates: Edit, this zone only). The token goes in
# a 0600 header file and the key in a body file, so neither is on a command line:
h=\$(mktemp) && chmod 0600 "\$h" && printf 'Authorization: Bearer %s\\nContent-Type: application/json\\n' "\$CLOUDFLARE_API_TOKEN" >"\$h"
b=\$(mktemp) && chmod 0600 "\$b" && jq -n --rawfile certificate $dir/client.pem --rawfile private_key $dir/client.key '{certificate: \$certificate, private_key: \$private_key}' >"\$b"
# 1. upload the client certificate (its status becomes "active" within minutes):
curl -sS -X POST -H @"\$h" --data-binary @"\$b" $API/zones/$zone/origin_tls_client_auth
# 2. check it:
curl -sS -H @"\$h" $API/zones/$zone/origin_tls_client_auth
# 3. zone-level AOP on (Cloudflare presents the uploaded certificate from then on):
curl -sS -X PUT -H @"\$h" --data '{"enabled":true}' $API/zones/$zone/origin_tls_client_auth/settings
rm -f "\$h" "\$b"
EOF
}

case "${1:-}" in
  generate) generate "${2:-}" ;;
  print) zone_ok "${2:-}"; [[ -n "${3:-}" ]] || die "usage: print <zone-id> <dir>"; print_calls "$2" "$3" ;;
  upload)
    zone_ok "${2:-}"; [[ -n "${3:-}" ]] || die "usage: upload <zone-id> <dir>"; dir_ok "$3"
    [[ -n "${CLOUDFLARE_API_TOKEN:-}" ]] || die "CLOUDFLARE_API_TOKEN isn't set (a token with Zone: SSL and Certificates: Edit, for this zone only)"
    b=$(body "$3"); trap 'rm -f "$b"' EXIT
    out=$(api POST "/zones/$2/origin_tls_client_auth" --data-binary @"$b")
    echo "uploaded: id $(jq -r '.result.id' <<<"$out"), status $(jq -r '.result.status' <<<"$out"), expires $(jq -r '.result.expires_on // "?"' <<<"$out")"
    echo "next: custom-origin-pull.sh status $2 until it is active, then enable $2" ;;
  status)
    zone_ok "${2:-}"
    api GET "/zones/$2/origin_tls_client_auth" | jq -r '.result[] | "\(.id)  \(.status)  expires \(.expires_on // "?")"'
    api GET "/zones/$2/origin_tls_client_auth/settings" | jq -r '"zone-level AOP enabled: \(.result.enabled)"' ;;
  enable)
    zone_ok "${2:-}"
    api PUT "/zones/$2/origin_tls_client_auth/settings" --data '{"enabled":true}' | jq -r '"zone-level AOP enabled: \(.result.enabled)"'
    echo "next, on the droplet: origin-pull-ca.sh --custom-only, then smoke.sh" ;;
  *) echo "usage: custom-origin-pull.sh generate [dir] | print <zone-id> <dir> | upload <zone-id> <dir> | status <zone-id> | enable <zone-id>" >&2; exit 2 ;;
esac
