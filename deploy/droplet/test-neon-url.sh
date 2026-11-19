#!/usr/bin/env bash
# Tests neon-url.sh's rewrite of the URL Neon's dashboard hands out.
#
#   deploy/droplet/test-neon-url.sh
set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=deploy/droplet/neon-url.sh
. ./neon-url.sh
fail() { echo "FAIL: $*" >&2; exit 1; }
check() { # <input> <expected>
  local got
  got=$(neon_url_fix "$1")
  [[ "$got" == "$2" ]] || fail "$1 → $got, wanted $2"
}
h=ep-quiet-bird-a1b2c3d4
check "postgresql://owner:pw@$h-pooler.us-east-2.aws.neon.tech/colophon?sslmode=require&channel_binding=require" \
      "postgresql://owner:pw@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full"
check "postgresql://owner:pw@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full" \
      "postgresql://owner:pw@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full"
check "postgres://owner:pw@$h.us-east-2.aws.neon.tech/colophon" \
      "postgres://owner:pw@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full"
check "postgresql://owner:pw@$h-pooler.us-east-2.aws.neon.tech/colophon?channel_binding=require&sslmode=require" \
      "postgresql://owner:pw@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full"
# A password that happens to contain "-pooler" is left alone: only the host is rewritten.
check "postgresql://owner:x-pooler.y@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full" \
      "postgresql://owner:x-pooler.y@$h.us-east-2.aws.neon.tech/colophon?sslmode=verify-full"
neon_url_fix "postgresql://o:p@$h-pooler.us-east-2.aws.neon.tech/colophon?sslmode=require&channel_binding=require" >/dev/null
(( ${#NEON_URL_NOTES[@]} == 3 )) || fail "notes: ${NEON_URL_NOTES[*]}"
echo "neon-url: the pooled host becomes the direct one, require and channel_binding become verify-full"
