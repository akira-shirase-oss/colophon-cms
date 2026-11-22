# shellcheck shell=bash
# Sourced by install.sh and its test: whether the backups' R2 credentials may be used (CREATE.md).
#
# The backups normally have their own token (Object Read & Write on colophon-backups only), so a compromised
# server, which holds the media token, can't delete or rewrite them. One token on both buckets is the operator's
# explicit, remembered choice: the file r2_backup_shares_media in the install directory holding "yes". It is
# accepted with a warning: a server compromise could then delete the backups the bucket lock no longer holds
# (older than its 7 days).

# r2_backup_shared <install-dir>: true when the operator chose one token for both buckets.
r2_backup_shared() {
  [[ -r "$1/r2_backup_shares_media" ]] && [[ "$(tr -d ' \t\r\n' <"$1/r2_backup_shares_media")" == yes ]]
}

# r2_backup_check <media-key-id> <backup-key-id> <media-secret-file> <backup-secret-file> <install-dir>:
# 0 when the backups' credentials may be used. A refusal, or the shared token's warning, goes to stderr.
r2_backup_check() {
  local media_id=$1 backup_id=$2 media_secret=$3 backup_secret=$4 dir=$5
  if r2_backup_shared "$dir"; then
    if [[ "$media_id" != "$backup_id" ]] || ! cmp -s "$media_secret" "$backup_secret"; then
      echo "refusing: r2_backup_shares_media=yes, but the backups' key id or secret isn't the media token's" >&2
      return 1
    fi
    echo "WARNING: the backups use the media R2 token (r2_backup_shares_media=yes): a server compromise could delete backups older than the bucket lock (7 days). Give the backups their own token when you can (CREATE.md)." >&2
    return 0
  fi
  if [[ "$media_id" == "$backup_id" ]]; then
    echo "refusing: the backups need their own R2 token, not the media one (CREATE.md); to accept one token on both buckets, write yes to $dir/r2_backup_shares_media" >&2
    echo "  (r2_backup_shares_media=yes: a server compromise could then delete backups older than the bucket lock)" >&2
    return 1
  fi
  if cmp -s "$media_secret" "$backup_secret"; then
    echo "refusing: the backups' R2 secret is the media one; make the backups their own token (CREATE.md), or set r2_backup_shares_media=yes" >&2
    return 1
  fi
}
