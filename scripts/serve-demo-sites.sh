#!/usr/bin/env bash
# Builds and serves the five Astro demo sites (apps/sites/README.md; D-133), each on its own
# port bound to localhost:
#
#   blog      http://127.0.0.1:58110   (collection posts)
#   docs      http://127.0.0.1:58111   (collection guides)
#   recipes   http://127.0.0.1:58112   (collection recipes)
#   news      http://127.0.0.1:58113   (collection stories)
#   portfolio http://127.0.0.1:58114   (collection photos)
#
#   scripts/serve-demo-sites.sh           build all five, then serve them (Ctrl-C stops them)
#   scripts/serve-demo-sites.sh build     build only
#   scripts/serve-demo-sites.sh serve     serve what's already built
#   scripts/serve-demo-sites.sh stop      stop sites this script started
#   scripts/serve-demo-sites.sh <site>    build and serve just that one site
#
# Each build reads the published content named by apps/sites/.colophon.json, written by
# scripts/seed-demo-sites.sh, so a site shows exactly one release. The dev stack and its server
# are only read, never touched.
set -euo pipefail
cd "$(dirname "$0")/.."

SITES=(blog:58110 docs:58111 recipes:58112 news:58113 portfolio:58114)
STATE=target/demo-sites
mkdir -p "$STATE"

site_names() { for s in "${SITES[@]}"; do echo "${s%%:*}"; done; }
port_of() { for s in "${SITES[@]}"; do [[ "${s%%:*}" == "$1" ]] && echo "${s##*:}" && return; done; }

build_one() {
  local site="$1" port
  port="$(port_of "$site")"
  if [[ -z "$port" ]]; then
    echo "error: unknown site '$site' (one of: $(site_names | tr '\n' ' '))" >&2
    exit 2
  fi
  echo "building @colophon/site-$site"
  npm run build --silent -w "@colophon/site-$site" >"$STATE/$site-build.log" 2>&1 || {
    echo "error: the $site build failed; see $STATE/$site-build.log" >&2
    exit 1
  }
}

serve_one() {
  local site="$1" port
  port="$(port_of "$site")"
  (cd "apps/sites/$site" && exec npx astro preview --port "$port" --host 127.0.0.1) \
    >"$STATE/$site.log" 2>&1 &
  echo "$!" >"$STATE/$site.pid"
}

stop_all() {
  for site in $(site_names); do
    if [[ -f "$STATE/$site.pid" ]]; then
      pid="$(cat "$STATE/$site.pid")"
      if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        echo "stopped $site (pid $pid)"
      fi
      rm -f "$STATE/$site.pid"
    fi
  done
}

command="${1:-all}"
case "$command" in
  build)
    for site in $(site_names); do build_one "$site"; done
    ;;
  serve)
    stop_all >/dev/null
    for site in $(site_names); do
      serve_one "$site"
    done
    ;;
  stop)
    stop_all
    ;;
  all)
    for site in $(site_names); do build_one "$site"; done
    stop_all >/dev/null
    pids=()
    for site in $(site_names); do
      serve_one "$site"
      pids+=("$(cat "$STATE/$site.pid")")
    done
    trap 'stop_all >/dev/null 2>&1 || true' EXIT
    echo ""
    for site in $(site_names); do
      echo "  $site → http://127.0.0.1:$(port_of "$site")/"
    done
    echo ""
    echo "Logs in $STATE/. Ctrl-C stops the sites."
    for pid in "${pids[@]}"; do
      wait "$pid" 2>/dev/null || true
    done
    ;;
  *)
    if port_of "$command" >/dev/null; then
      build_one "$command"
      port="$(port_of "$command")"
      echo "  $command → http://127.0.0.1:$port/  (Ctrl-C stops it)"
      cd "apps/sites/$command"
      exec npx astro preview --port "$port" --host 127.0.0.1
    else
      echo "usage: $0 [build|serve|stop|all|$(site_names | tr '\n' '|')]" >&2
      exit 2
    fi
    ;;
esac
