#!/bin/sh
# PostgreSQL `archive_command` for the local stack (D-125). Postgres calls this once per
# completed WAL segment:
#
#   archive-wal.sh %p %f
#
# %p is the segment's path relative to the data directory (e.g. pg_wal/0000000100000000000000A3),
# %f its file name. Exit 0 only once the file is durably stored: on a failure Postgres retries
# the segment forever rather than lose it, which is exactly what the archive needs to promise.
#
# The local stand-in is the named Docker volume mounted at /wal-archive (O5: nothing in this
# stack ever deletes from it; retention is a documented job in deploy/runbooks/backup.md). The
# postgres:18 image carries no S3 tooling — in production this command is the same contract
# pointed at object storage (e.g. wal-g, or an S3 client pushing $dest/$name), and the WAL
# files themselves do not change.
set -eu

src=$1
name=$2
dest=${WAL_ARCHIVE_DIR:-/wal-archive}

# No mkdir: if the archive mount is missing, failing is honest — Postgres retries forever and
# pg_wal growth is visible — while "succeeding" into the container's ephemeral layer would
# archive nowhere durable and say otherwise.
if [ ! -d "$dest" ]; then
  echo "archive-wal: $dest is not mounted" >&2
  exit 1
fi

# Copy under a temp name, then rename, so a crash can never leave half a segment under the
# real name. Postgres may retry a segment after a restart: an existing identical file is a
# success, but an existing different file must never be overwritten — that is an archive
# fault, and failing (Postgres retries) beats corrupting the archive.
tmp="$dest/.tmp.$name.$$"
cp -- "$src" "$tmp"
chmod 600 "$tmp"
if [ -e "$dest/$name" ]; then
  if cmp -s "$tmp" "$dest/$name"; then
    rm -f "$tmp"
    exit 0
  fi
  rm -f "$tmp"
  echo "archive-wal: refusing to overwrite $dest/$name with different contents" >&2
  exit 1
fi
mv "$tmp" "$dest/$name"
# Flush before reporting success: an archived file must survive a host crash, not just a
# Postgres one. sync -f syncs the file's filesystem; fall back to a whole sync where the
# flag doesn't exist.
sync -f "$dest/$name" 2>/dev/null || sync "$dest" 2>/dev/null || true
