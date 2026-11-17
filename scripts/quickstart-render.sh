#!/usr/bin/env bash
# Writes the quick start's compose.yaml for a release (D-174): every Colophon service's default
# image becomes <image>@<digest>, so `docker compose up` runs exactly the image the release
# built. COLOPHON_IMAGE still overrides it.
#
#   scripts/quickstart-render.sh ghcr.io/akira-shirase-oss/colophon:0.1.0@sha256:... deploy/quickstart/compose.yaml >compose.yaml
set -euo pipefail
if [[ $# -ne 2 || "$1" != *@sha256:* ]]; then
  echo "usage: scripts/quickstart-render.sh <image>@sha256:<digest> <compose.yaml>" >&2
  exit 2
fi
ref=$1 file=$2
# shellcheck disable=SC2016 # a literal ${...}: compose's syntax, matched, never expanded
pattern='\${COLOPHON_IMAGE:-[^}]*}'
found=$(grep -c "$pattern" "$file" || true)
# roles, migrate and colophon: three services run the Colophon image.
if [[ "$found" -ne 3 ]]; then
  echo "expected 3 Colophon image references in $file, found $found" >&2
  exit 1
fi
sed "s|$pattern|\${COLOPHON_IMAGE:-$ref}|" "$file"
