# shellcheck shell=bash
# Sourced by install.sh, smoke.sh and their test: the droplet's hostname is ONE setting,
# COLOPHON_PUBLIC_URL in /etc/colophon/colophon.env. Caddy's site address and the smoke check's
# domain are derived from it, so moving to another host is one env line and a re-run of install.sh.
#
# The name is one level under the zone (colophonapp.404human.dev, not app.colophon.404human.dev):
# Cloudflare's free Universal SSL covers the apex and *.zone only; a deeper name needs Advanced
# Certificate Manager (CREATE.md, row 7).

# The site address the shipped Caddyfile carries, replaced at install by the env's host.
PUBLIC_HOST_SHIPPED=colophonapp.404human.dev

# public_url <env-file>: prints the file's (last) COLOPHON_PUBLIC_URL without a trailing slash,
# or fails unless it is exactly https://<host> — a lowercase DNS name, no port, path, query or
# credentials (Caddy and Cloudflare serve 443; anything else here is a typo).
public_url() {
  local file=$1 url host
  [[ -r "$file" ]] || { echo "error: can't read $file" >&2; return 1; }
  url=$(sed -n 's/^COLOPHON_PUBLIC_URL=//p' "$file" | tail -1 | tr -d '\r')
  url=${url%/}
  [[ -n "$url" ]] || { echo "error: $file has no COLOPHON_PUBLIC_URL" >&2; return 1; }
  [[ "$url" == https://* ]] || { echo "error: COLOPHON_PUBLIC_URL must be https://<host>, not '$url'" >&2; return 1; }
  host=${url#https://}
  public_host_valid "$host" || { echo "error: COLOPHON_PUBLIC_URL's host '$host' isn't a plain lowercase hostname (no port, path or credentials)" >&2; return 1; }
  printf '%s' "$url"
}

# public_host <env-file>: the host of the file's COLOPHON_PUBLIC_URL (as public_url checks it).
public_host() {
  local url
  url=$(public_url "$1") || return 1
  printf '%s' "${url#https://}"
}

# public_host_valid <host>: a DNS name of at least two labels, each 1-63 of [a-z0-9-] not starting
# or ending with '-', at most 253 characters in all.
public_host_valid() {
  local host=$1 label='[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?'
  ((${#host} <= 253)) && [[ "$host" =~ ^($label\.)+$label$ ]]
}

# caddyfile_for_host <caddyfile> <host>: prints the Caddyfile with its one shipped site address
# line replaced by <host>'s; fails if the host is odd or the file doesn't hold exactly that line.
caddyfile_for_host() {
  local file=$1 host=$2 line count
  public_host_valid "$host" || { echo "error: '$host' isn't a plain lowercase hostname" >&2; return 1; }
  line="$PUBLIC_HOST_SHIPPED:443 {"
  count=$(grep -cxF "$line" "$file" || true)
  [[ "$count" == 1 ]] || { echo "error: $file has $count lines '$line', wanted exactly 1" >&2; return 1; }
  while IFS= read -r l || [[ -n "$l" ]]; do
    if [[ "$l" == "$line" ]]; then printf '%s:443 {\n' "$host"; else printf '%s\n' "$l"; fi
  done <"$file"
}

# droplet_domain <env-file>: the host smoke.sh checks when SMOKE_DOMAIN doesn't say: the env's
# COLOPHON_PUBLIC_URL host when the file sets one (failing if it's odd), else the shipped name.
droplet_domain() {
  local file=$1
  if grep -q '^COLOPHON_PUBLIC_URL=' "$file" 2>/dev/null; then public_host "$file"; else printf '%s' "$PUBLIC_HOST_SHIPPED"; fi
}
