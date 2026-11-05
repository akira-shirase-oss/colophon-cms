#!/usr/bin/env bash
# Seeds the five Astro demo sites (apps/sites/README.md; D-133) against a running Colophon
# server: one `demo-sites` project with the `posts`, `guides`, `recipes`, `stories` and `photos`
# collections, generated photographs through the real upload and verify pipeline, and every
# entry published as release 1 through the changeset flow. Writes `apps/sites/.colophon.json`,
# which the sites and `scripts/serve-demo-sites.sh` read.
#
#   scripts/seed-demo-sites.sh [origin]
#
# `origin` defaults to the development server on http://localhost:58000. Every run creates a new
# project, so re-running is safe: the sites build from the newest run's record.
set -euo pipefail
cd "$(dirname "$0")/.."

for tool in node curl; do
  command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
done

origin="${1:-http://localhost:58000}"
if ! curl -s -o /dev/null "$origin/readyz"; then
  echo "error: no server answers at $origin; start it first (see deploy/README.md)" >&2
  exit 1
fi

exec node scripts/seed-demo-sites.mts "$origin"
