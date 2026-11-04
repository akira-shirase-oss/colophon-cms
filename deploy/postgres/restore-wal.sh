#!/bin/sh
# PostgreSQL `restore_command` for the local stack (D-125), the mirror of archive-wal.sh:
# during recovery Postgres calls
#
#   restore-wal.sh %f %p
#
# asking for archived segment %f to appear at %p. A missing file exits non-zero, which tells
# recovery the archive is exhausted — that is how "recover as far as the archive goes" ends.
#
# Locally it reads the named Docker volume archive-wal.sh writes; in production the same
# contract reads the object storage the production archive_command writes (wal-g restore, or
# an S3 fetch). The full restore procedure — clean infrastructure, a new recovery epoch, the
# deletion-ledger replay, reconciling external effects — is a later P6 slice; this command is
# only the segment fetch PITR needs at the bottom of it.
set -eu

name=$1
dst=$2
src=${WAL_ARCHIVE_DIR:-/wal-archive}/$name

[ -f "$src" ] || exit 1
cp -- "$src" "$dst"
