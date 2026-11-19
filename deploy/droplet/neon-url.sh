# shellcheck shell=bash
# Sourced by install.sh (and its test): Neon's dashboard hands out the POOLED host with
# ?sslmode=require&channel_binding=require. Colophon needs the DIRECT host (LISTEN and session
# advisory locks don't survive a transaction pooler) and verify-full (D-156: require takes any
# certificate; channel_binding is refused rather than ignored).
#
# neon_url_fix <url>: prints the fixed URL; NEON_URL_NOTES lists what changed.
neon_url_fix() {
  local url=$1
  NEON_URL_NOTES=()
  if [[ "$url" =~ ^(postgres(ql)?://[^@]+@[^/.]+)-pooler(\.[^/]+/.*)$ ]]; then
    url="${BASH_REMATCH[1]}${BASH_REMATCH[3]}"
    NEON_URL_NOTES+=("the -pooler host became the direct one (LISTEN and session advisory locks need it)")
  fi
  if [[ "$url" == *channel_binding=* ]]; then
    url=$(sed -E 's/([?&])channel_binding=[^&]*&?/\1/; s/[?&]$//' <<<"$url")
    NEON_URL_NOTES+=("channel_binding dropped (Colophon refuses it rather than ignore it)")
  fi
  if [[ "$url" == *sslmode=* ]]; then
    if [[ "$url" != *sslmode=verify-full* ]]; then
      url=$(sed -E 's/sslmode=[a-z-]+/sslmode=verify-full/' <<<"$url")
      NEON_URL_NOTES+=("sslmode set to verify-full")
    fi
  else
    url="$url$([[ "$url" == *\?* ]] && echo '&' || echo '?')sslmode=verify-full"
    NEON_URL_NOTES+=("sslmode=verify-full added")
  fi
  printf '%s' "$url"
}
