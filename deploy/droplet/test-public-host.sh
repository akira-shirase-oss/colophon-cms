#!/usr/bin/env bash
# Tests public-host.sh: the hostname is one setting (COLOPHON_PUBLIC_URL), and Caddy's site address
# and the smoke check's domain follow it, refusing anything that isn't a plain https://<host>.
#
#   deploy/droplet/test-public-host.sh
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=deploy/droplet/public-host.sh
. ./public-host.sh
fail() { echo "FAIL: $*" >&2; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
env_with() { printf '# a comment\nCOLOPHON_LISTEN_ADDR=127.0.0.1:58000\nCOLOPHON_PUBLIC_URL=%s\n' "$1" >"$tmp/env"; }

# The shipped template and Caddyfile agree with each other and with the shipped name.
[[ "$(public_host colophon.env.template)" == "$PUBLIC_HOST_SHIPPED" ]] || fail "the template's host isn't $PUBLIC_HOST_SHIPPED"
[[ "$(grep -cxF "$PUBLIC_HOST_SHIPPED:443 {" Caddyfile)" == 1 ]] || fail "the Caddyfile doesn't carry $PUBLIC_HOST_SHIPPED:443 once"

# Good values: the host, without a trailing slash; the last line wins, as systemd's EnvironmentFile.
env_with https://cms.example.org/
[[ "$(public_url "$tmp/env")" == https://cms.example.org ]] || fail "trailing slash kept"
[[ "$(public_host "$tmp/env")" == cms.example.org ]] || fail "host of https://cms.example.org/"
printf 'COLOPHON_PUBLIC_URL=https://other.example.org\n' >>"$tmp/env"
[[ "$(public_host "$tmp/env")" == other.example.org ]] || fail "the last COLOPHON_PUBLIC_URL should win"

# Odd values are refused, never passed on to Caddy.
for bad in "" "http://cms.example.org" "https://" "https://cms" "https://CMS.example.org" "https://cms.example.org:8443" \
  "https://cms.example.org/studio" "https://user@cms.example.org" "https://-cms.example.org" "https://cms-.example.org" \
  "https://cms..example.org" "https://cms.example.org?x=1" "https://cms example.org" "https://cms.example.org;x" \
  "https://cms.example.org\"" "'https://cms.example.org'"; do
  env_with "$bad"
  if out=$(public_host "$tmp/env" 2>/dev/null); then fail "accepted '$bad' as '$out'"; fi
done
: >"$tmp/empty"
if public_host "$tmp/empty" 2>/dev/null; then fail "a file without COLOPHON_PUBLIC_URL was accepted"; fi
if public_host "$tmp/missing" 2>/dev/null; then fail "a missing file was accepted"; fi

# The Caddyfile: exactly the site address line changes, everything else byte for byte.
caddyfile_for_host Caddyfile cms.example.org >"$tmp/Caddyfile"
diff <(grep -vxF "$PUBLIC_HOST_SHIPPED:443 {" Caddyfile) <(grep -vxF "cms.example.org:443 {" "$tmp/Caddyfile") >/dev/null \
  || fail "the rendered Caddyfile differs beyond its site address"
[[ "$(grep -cxF "cms.example.org:443 {" "$tmp/Caddyfile")" == 1 ]] || fail "the site address wasn't replaced"
grep -qF "$PUBLIC_HOST_SHIPPED" "$tmp/Caddyfile" && fail "the shipped name survived the rendering"
cmp -s Caddyfile <(caddyfile_for_host Caddyfile "$PUBLIC_HOST_SHIPPED") || fail "rendering the shipped host changed the file"
if caddyfile_for_host Caddyfile 'cms.example.org { respond 200 }' >/dev/null 2>&1; then fail "an odd host was rendered"; fi
if caddyfile_for_host Caddyfile "" >/dev/null 2>&1; then fail "an empty host was rendered"; fi
grep -vxF "$PUBLIC_HOST_SHIPPED:443 {" Caddyfile >"$tmp/none"
if caddyfile_for_host "$tmp/none" cms.example.org >/dev/null 2>&1; then fail "a Caddyfile without the site address line was rendered"; fi
cat Caddyfile Caddyfile >"$tmp/two"
if caddyfile_for_host "$tmp/two" cms.example.org >/dev/null 2>&1; then fail "a Caddyfile with two site address lines was rendered"; fi

# The smoke check's domain: the env's host, the shipped name without one, a refusal when odd.
env_with https://cms.example.org
[[ "$(droplet_domain "$tmp/env")" == cms.example.org ]] || fail "smoke domain from the env"
[[ "$(droplet_domain "$tmp/missing")" == "$PUBLIC_HOST_SHIPPED" ]] || fail "smoke domain without an env file"
env_with https://cms.example.org:8443
if droplet_domain "$tmp/env" >/dev/null 2>&1; then fail "smoke took an odd COLOPHON_PUBLIC_URL"; fi
echo "public-host: one setting, COLOPHON_PUBLIC_URL; Caddy's site address and smoke's domain follow it, odd values refused"
