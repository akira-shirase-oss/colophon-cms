#!/usr/bin/env bash
# D-223: how many sequential database round trips one request costs, per endpoint. On Neon every statement is a
# round trip, so this count times the server-Postgres RTT is the floor of each request's latency.
#
#   perf/saturation/round-trips.sh <base-url> <postgres container> [repeats, default 5]
#
# Against a stack of your own only: it turns on log_statement=all for the run (reset after). Requests go one at a
# time, and between two requests it sends a marker statement on a connection of its own, so every statement Postgres
# logs between two markers belongs to that request; background work (the worker's claims, the change-hint drain,
# the sweeps) is recognised and left out. Prints, per endpoint, the median statements per request (each a round
# trip), the transactions among them, and the release checks (D-217's statement after each transaction).
# ROUND_TRIPS_KEEP=<path> keeps the statement log for a full breakdown.
set -euo pipefail
base=$1 pg=$2 repeats=${3:-5}
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'psql_owner -c "ALTER SYSTEM RESET log_statement" -c "ALTER SYSTEM RESET log_line_prefix" -c "SELECT pg_reload_conf()" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT

psql_owner() { docker exec -i "$pg" sh -c 'PGPASSWORD=$(cat "$POSTGRES_PASSWORD_FILE") psql -U colophon -d colophon -XAtq "$@"' sh "$@"; }
mark() { psql_owner -c "SELECT 'rt-marker:$1'" >/dev/null; }
key() { echo "rt-$(date +%s)-$RANDOM$RANDOM-$1"; }

jar=$work/jar
curl -fsS -c "$jar" -o /dev/null -X POST "$base/auth/signin" -H 'Content-Type: application/json' -H "Origin: $base" \
  -d '{"email":"dev-owner@colophon.test","password":"colophon-dev"}'
csrf=$(awk '$6 == "colophon_csrf" {print $7}' "$jar")
w=(-b "$jar" -H "X-CSRF-Token: $csrf" -H "Origin: $base" -H 'Content-Type: application/json')
project=$(curl -fsS "${w[@]}" -X POST "$base/api/v1/projects" -d "{\"slug\":\"rt-$(date +%s)$RANDOM\",\"name\":\"round trips\"}" | jq -r '.data.id')
proj="$base/api/v1/projects/$project"
env="$proj/environments/production"
body() { echo "{\"title\":\"$1\",\"body\":{\"schema\":\"colophon/rich-text\",\"version\":1,\"doc\":{\"type\":\"doc\",\"content\":[{\"type\":\"paragraph\",\"content\":[{\"type\":\"text\",\"text\":\"$1\"}]}]}}}"; }

psql_owner -c "ALTER SYSTEM SET log_statement = 'all'" -c "ALTER SYSTEM SET log_line_prefix = '%m [%p] %v '" -c "SELECT pg_reload_conf()" >/dev/null
sleep 1
since=$(date -u +%s)

entry=""
etag=""
generation=0
for n in $(seq "$repeats"); do
  mark "entry-create:$n"
  created=$(curl -fsS -D "$work/h" "${w[@]}" -H "Idempotency-Key: $(key "create-$n")" -X POST "$env/collections/articles/entries" -d "$(body "Round trip $n")")
  entry=$(jq -r '.data.id' <<<"$created"); etag=$(sed -n 's/^[Ee][Tt]ag: //p' "$work/h" | tr -d '\r')
  mark "entry-read:$n"
  curl -fsS -D "$work/h" -o /dev/null -b "$jar" "$env/collections/articles/entries/$entry"
  mark "entry-save:$n"
  curl -fsS -D "$work/h" -o /dev/null "${w[@]}" -H "If-Match: $etag" -H "Idempotency-Key: $(key "save-$n")" -X PUT "$env/collections/articles/entries/$entry" -d "$(body "Round trip $n saved")"
  # Three requests between these two markers: the changeset, its submit and its review.
  mark "changeset-submit-review:$n"
  set=$(curl -fsS "${w[@]}" -H "Idempotency-Key: $(key "set-$n")" -X POST "$env/changesets" -d "{\"title\":\"rt $n\",\"entries\":[\"$entry\"]}")
  path="$env/changesets/$(jq -r '.data.id' <<<"$set")"; hash=$(jq -r '.data.hash' <<<"$set")
  curl -fsS -o /dev/null "${w[@]}" -H "If-Match: \"$hash\"" -X POST "$path/submit"
  curl -fsS -o /dev/null "${w[@]}" -H "If-Match: \"$hash\"" -X POST "$path/reviews" -d '{"decision":"approve"}'
  mark "publish:$n"
  curl -fsS -o /dev/null "${w[@]}" -H "If-Match: \"$hash\"" -H "Idempotency-Key: $(key "pub-$n")" -X POST "$path/publish" -d "{\"base_generation\":$generation}"
  generation=$(( generation + 1 ))
  mark "delivery-read:$n"
  curl -fsS -o /dev/null "$base/content/v1/$project/production/articles"
  mark "members-list:$n"
  curl -fsS -o /dev/null -b "$jar" "$proj/members"
  mark "end:$n"
done
sleep 1
docker logs --since "$since" "$pg" >"$work/pg.log" 2>&1
# ROUND_TRIPS_KEEP=<path>: keep the statement log, for a path's full breakdown.
[[ -z ${ROUND_TRIPS_KEEP:-} ]] || cp "$work/pg.log" "$ROUND_TRIPS_KEEP"
python3 -I "$here/round-trips.py" "$work/pg.log"
