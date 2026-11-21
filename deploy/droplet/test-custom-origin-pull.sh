#!/usr/bin/env bash
# Tests custom-origin-pull.sh against a stand-in curl that records its command line and answers
# like Cloudflare's API:
#   - generate makes a 0700 directory whose client certificate verifies against its CA for client
#     authentication, the keys 0600, and refuses a directory that exists;
#   - print names the three API calls and no secret;
#   - upload refuses without CLOUDFLARE_API_TOKEN or with an odd zone id, and with one sends the
#     certificate and key to the zone's endpoint, while neither the token nor the key is on any
#     command line, and no temporary file is left behind.
#
#   deploy/droplet/test-custom-origin-pull.sh
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
zone=0123456789abcdef0123456789abcdef
token=cf-token-$RANDOM-secret

./custom-origin-pull.sh generate "$work/aop" >"$work/gen.out"
mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"; }
[[ "$(mode "$work/aop")" == 700 ]] || fail "the directory is $(mode "$work/aop"), not 700"
for k in client.key ca.key; do [[ "$(mode "$work/aop/$k")" == 600 ]] || fail "$k is $(mode "$work/aop/$k"), not 600"; done
openssl verify -CAfile "$work/aop/ca.pem" -purpose sslclient "$work/aop/client.pem" >/dev/null || fail "the client certificate doesn't verify for client auth"
if ./custom-origin-pull.sh generate "$work/aop" >/dev/null 2>&1; then fail "generate overwrote an existing directory"; fi
if grep -q 'PRIVATE KEY' "$work/gen.out"; then fail "generate printed a key"; fi
echo "  ok    generate: a 0700 directory, 0600 keys, a client certificate its CA verifies; an existing directory refused"

./custom-origin-pull.sh print "$zone" "$work/aop" >"$work/print.out"
for p in "POST .*zones/$zone/origin_tls_client_auth" "zones/$zone/origin_tls_client_auth\$" "PUT .*zones/$zone/origin_tls_client_auth/settings"; do
  grep -qE "$p" "$work/print.out" || fail "print lacks $p"
done
if grep -q 'PRIVATE KEY' "$work/print.out"; then fail "print printed the key"; fi
echo "  ok    print: upload, check and enable, with no secret"

# The stand-in curl: each command line to calls.log, then Cloudflare's answer.
mkdir -p "$work/bin"
cat >"$work/bin/curl" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$CALLS"
for a in "$@"; do case "$a" in @*) f=${a#@}; [[ -r "$f" ]] && cat "$f" >>"$CALLS.files" ;; esac; done
echo '{"success":true,"result":{"id":"cert-1","status":"pending_deployment","expires_on":"2031-01-01"}}'
STUB
chmod +x "$work/bin/curl"
up() { env PATH="$work/bin:$PATH" CALLS="$work/calls.log" TMPDIR="$work/tmp" ./custom-origin-pull.sh upload "$@"; }
mkdir -p "$work/tmp"
if env -u CLOUDFLARE_API_TOKEN PATH="$work/bin:$PATH" CALLS="$work/calls.log" ./custom-origin-pull.sh upload "$zone" "$work/aop" >/dev/null 2>&1; then
  fail "upload ran without CLOUDFLARE_API_TOKEN"
fi
if CLOUDFLARE_API_TOKEN=$token up nope "$work/aop" >/dev/null 2>&1; then fail "upload took an odd zone id"; fi
[[ ! -s "$work/calls.log" ]] || fail "a refused upload still called the API"
CLOUDFLARE_API_TOKEN=$token up "$zone" "$work/aop" >"$work/up.out" || fail "upload: $(cat "$work/up.out")"
grep -q "POST .*zones/$zone/origin_tls_client_auth" "$work/calls.log" || fail "upload didn't POST to the zone: $(cat "$work/calls.log")"
if grep -qF "$token" "$work/calls.log"; then fail "the token was on curl's command line"; fi
if grep -q 'PRIVATE KEY' "$work/calls.log"; then fail "the key was on curl's command line"; fi
grep -qF "$token" "$work/calls.log.files" || fail "the token wasn't sent (from a file)"
key_line=$(sed -n 2p "$work/aop/client.key")
grep -qF "$key_line" "$work/calls.log.files" || fail "the key wasn't sent (from a file)"
[[ -z "$(ls -A "$work/tmp")" ]] || fail "temporary files left: $(ls -A "$work/tmp")"
grep -q 'uploaded: id cert-1' "$work/up.out" || fail "upload's report: $(cat "$work/up.out")"
echo "  ok    upload: refused without a token or with an odd zone; sent from files, never argv; nothing left behind"
echo "custom origin pull: generate, print and upload keep the token and the key off every command line"
