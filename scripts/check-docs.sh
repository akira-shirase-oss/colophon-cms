#!/usr/bin/env bash
# The docs check (D-241): what a push that changes only docs/, *.md or perf/results/ runs in CI,
# in seconds. No home directory in a committed document or result (the machine's owner stays out of
# the repository), no merge conflict left in any tracked file, and every relative link in a Markdown
# file under docs/ (and the top-level ones) names a file that exists.
set -euo pipefail
cd "$(dirname "$0")/.."

failed=0

# Home directories: /Users/<name> or /home/<name>/ (GitHub's /home/runner/ is no one's).
if git grep -nE '/Users/[A-Za-z]|/home/[a-z][a-z0-9_-]*/' -- 'docs/**' '*.md' 'perf/results/**' \
  | grep -v '/home/runner/'; then
  echo "error: a home directory in a committed document or result (above)" >&2
  failed=1
fi

# Merge conflicts left behind.
if git grep -nE '^(<<<<<<< |>>>>>>> )' -- . ':!*.png' ':!*.jpg' ':!*.woff2'; then
  echo "error: merge conflict markers (above)" >&2
  failed=1
fi

# Relative links: [text](path) and [text](path#anchor), outside code spans and fences.
broken=$(git ls-files -- 'docs/*.md' 'docs/**/*.md' '*.md' | while IFS= read -r file; do
  dir=$(dirname "$file")
  awk '/^```/ {fence = !fence; next} !fence {gsub(/`[^`]*`/, ""); print}' "$file" \
    | grep -oE '\]\([^)[:space:]]+\)' | sed -E 's/^\]\(//; s/\)$//' | while IFS= read -r target; do
      # (No `case` here: bash 3.2 can't parse one inside $(...).)
      [[ "$target" =~ ^(https?:|mailto:|#|/) ]] && continue
      path=${target%%#*}
      [[ -z "$path" ]] && continue
      [[ -e "$dir/$path" ]] || echo "$file: $target"
    done
done)
if [[ -n "$broken" ]]; then
  echo "$broken"
  echo "error: relative links to files that don't exist (above)" >&2
  failed=1
fi

if (( failed == 0 )); then
  echo "docs check: ok"
fi
exit "$failed"
