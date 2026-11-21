#!/usr/bin/env bash
# cloudflare-ips.sh against a stub ufw, run twice as an install and its re-run do: the second run
# finds the first one's 443 rules and replaces them. Before 2026-11-21 that died: `yes | ufw delete`
# under pipefail ends in SIGPIPE (141) once ufw has read its one answer, so a re-run of install.sh
# stopped after deleting one rule. The stub reads one answer from stdin when not given --force,
# as ufw does. Also checks SSH's rule is left alone, Caddy gets the ranges, and the server trusts
# loopback alone (Caddy tells it Cf-Connecting-Ip; trusting Cloudflare's ranges in the server let a
# request from a Cloudflare address choose its client address): an older run's Cloudflare list in
# colophon.env is replaced, a file without the setting gets it, and a list that isn't address ranges
# writes nothing.
# Runs on macOS's bash 3.2 and on Linux.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin" "$work/cf"

printf '%s\n' 173.245.48.0/20 103.21.244.0/22 103.22.200.0/22 103.31.4.0/22 141.101.64.0/18 108.162.192.0/18 \
  190.93.240.0/20 188.114.96.0/20 197.234.240.0/22 198.41.128.0/17 162.158.0.0/15 104.16.0.0/13 \
  104.24.0.0/14 172.64.0.0/13 131.0.72.0/22 >"$work/cf/ips-v4"
printf '%s\n' 2400:cb00::/32 2606:4700::/32 2803:f800::/32 2405:b500::/32 2405:8100::/32 2a06:98c0::/29 \
  2c0f:f248::/32 >"$work/cf/ips-v6"
ranges=22

cat >"$work/bin/ufw" <<'STUB'
#!/usr/bin/env bash
# A stand-in for ufw: rules are lines in $UFW_STUB_RULES.
set -eu
rules=$UFW_STUB_RULES
case "$1" in
  status)
    if [[ "${2:-}" == numbered ]]; then
      n=0
      while IFS= read -r rule; do n=$((n + 1)); printf '[%2d] %s\n' "$n" "$rule"; done <"$rules"
    else
      cat "$rules"
    fi ;;
  --force)
    [[ "$2" == delete ]] && sed -i.bak "${3}d" "$rules" && rm -f "$rules.bak" ;;
  delete)
    # ufw asks "Proceed with operation (y|n)?" and reads one answer, then exits.
    IFS= read -r answer || answer=n
    if [[ "$answer" == y* ]]; then sed -i.bak "${2}d" "$rules" && rm -f "$rules.bak"; fi ;;
  allow)
    from=""
    while [[ $# -gt 0 ]]; do [[ "$1" == from ]] && from=$2; shift; done
    printf '443/tcp                    ALLOW IN    %s                 # cloudflare\n' "$from" >>"$rules" ;;
  *) echo "stub ufw: unexpected $*" >&2; exit 2 ;;
esac
STUB
chmod +x "$work/bin/ufw"

export PATH="$work/bin:$PATH" UFW_STUB_RULES="$work/rules"
export COLOPHON_CLOUDFLARE_IPS_TEST=1 COLOPHON_CLOUDFLARE_IPS_FROM="$work/cf"
export COLOPHON_CADDY_TRUSTED_FILE="$work/cloudflare-trusted.caddy" COLOPHON_ENV_FILE="$work/colophon.env"
printf '22/tcp                     ALLOW IN    Anywhere                   # ssh: maintainer\n' >"$work/rules"
# As an older cloudflare-ips.sh left it: Cloudflare's ranges trusted by the server.
printf 'COLOPHON_LISTEN_ADDR=127.0.0.1:58000\nCOLOPHON_TRUSTED_PROXIES=127.0.0.1/32,173.245.48.0/20,2400:cb00::/32\n' >"$COLOPHON_ENV_FILE"

failures=0
# check <what> <wanted> <got>
check() {
  if [[ "$3" == "$2" ]]; then echo "  ok    $1"; else echo "  FAIL  $1: wanted '$2', got '$3'"; failures=$((failures + 1)); fi
}
count() { grep -c "$1" "$2" || true; }

echo "cloudflare-ips.sh against a stub ufw:"
for run in first second; do
  code=0
  "$here/cloudflare-ips.sh" >"$work/out" 2>&1 || code=$?
  check "the $run run exits 0 ($(tail -1 "$work/out"))" 0 "$code"
  check "after the $run run: one rule for 443 per range" "$ranges" "$(count '^443/tcp' "$work/rules")"
  check "after the $run run: SSH's rule is left alone" 1 "$(count '^22/tcp .*# ssh: maintainer' "$work/rules")"
done
check "Caddy trusts every range" "$ranges" "$(tr ' ' '\n' <"$COLOPHON_CADDY_TRUSTED_FILE" | grep -c /)"
check "Colophon trusts loopback alone, on one line (the older Cloudflare list replaced)" "COLOPHON_TRUSTED_PROXIES=127.0.0.1/32" \
  "$(grep '^COLOPHON_TRUSTED_PROXIES=' "$COLOPHON_ENV_FILE")"
check "the env file's other settings are kept" 1 "$(count '^COLOPHON_LISTEN_ADDR=127.0.0.1:58000$' "$COLOPHON_ENV_FILE")"

# A file without the setting gets loopback added.
printf 'COLOPHON_LOG=info\n' >"$COLOPHON_ENV_FILE"
"$here/cloudflare-ips.sh" >/dev/null 2>&1 || true
check "a file without COLOPHON_TRUSTED_PROXIES gets loopback alone" "COLOPHON_TRUSTED_PROXIES=127.0.0.1/32" \
  "$(grep '^COLOPHON_TRUSTED_PROXIES=' "$COLOPHON_ENV_FILE")"

# A list that isn't address ranges: nothing written, ufw untouched.
cp "$work/rules" "$work/rules.before"; rm -f "$COLOPHON_CADDY_TRUSTED_FILE"
printf '173.245.48.0/20\n<html>\n' >"$work/cf/ips-v4"
code=0; "$here/cloudflare-ips.sh" >/dev/null 2>&1 || code=$?
check "a list with '<html>' is refused" 1 "$code"
check "a refused list writes no Caddy file" "absent" "$([[ -e "$COLOPHON_CADDY_TRUSTED_FILE" ]] && echo present || echo absent)"
check "a refused list leaves ufw alone" "same" "$(cmp -s "$work/rules" "$work/rules.before" && echo same || echo changed)"

if [[ "$failures" -gt 0 ]]; then
  echo "test-cloudflare-ips: $failures failed" >&2
  exit 1
fi
echo "test-cloudflare-ips: all passed"
