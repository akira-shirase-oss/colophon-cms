# shellcheck shell=bash
# The backup format's encryption helpers, shared by scripts/backup.sh (writes) and
# scripts/restore.sh (reads). Source this file; it sets ENC, key_file and secret_ref.
#
# Encryption is age when age and age-keygen are on PATH, else gpg --symmetric (AES-256). A
# restore overrides ENC with the backup manifest's encryption.tool so a machine that gained
# age since the backup still opens a gpg file. The secret always comes from a file, never a
# flag: COLOPHON_BACKUP_PASSPHRASE_FILE or COLOPHON_BACKUP_AGE_KEY_FILE wins, else
# deploy/backup-passphrase.local / deploy/backup-age-key.local (mode 600, gitignored).

ENC=""
if command -v age >/dev/null 2>&1 && command -v age-keygen >/dev/null 2>&1; then ENC=age; else ENC=gpg; fi

# secret_ref: what manifest.json records — where the key came from, never the key itself.
# shellcheck disable=SC2034 # read by the scripts that source this file (backup.sh's manifest)
secret_ref=""
key_file=""
ensure_key() { # create=1 lets a first run mint the local dev secret; verify must find one.
  if [[ "$ENC" == age ]]; then
    key_file="${COLOPHON_BACKUP_AGE_KEY_FILE:-deploy/backup-age-key.local}"
    if [[ ! -f "$key_file" ]]; then
      if [[ "${1:-}" != "create" ]]; then
        echo "error: no age identity at $key_file (set COLOPHON_BACKUP_AGE_KEY_FILE)" >&2; exit 1
      fi
      age-keygen -o "$key_file" 2>/dev/null
      chmod 600 "$key_file"
      echo "note: generated $key_file (gitignored, mode 600) — losing it loses every backup" >&2
    fi
    secret_ref="age identity ${COLOPHON_BACKUP_AGE_KEY_FILE:+COLOPHON_BACKUP_AGE_KEY_FILE=}$key_file"
  else
    key_file="${COLOPHON_BACKUP_PASSPHRASE_FILE:-deploy/backup-passphrase.local}"
    if [[ ! -f "$key_file" ]]; then
      if [[ "${1:-}" != "create" ]]; then
        echo "error: no passphrase file at $key_file (set COLOPHON_BACKUP_PASSPHRASE_FILE)" >&2; exit 1
      fi
      openssl rand -base64 48 >"$key_file"
      chmod 600 "$key_file"
      echo "note: generated $key_file (gitignored, mode 600) — losing it loses every backup" >&2
    fi
    # shellcheck disable=SC2034 # read by the scripts that source this file (backup.sh's manifest)
    secret_ref="gpg passphrase file ${COLOPHON_BACKUP_PASSPHRASE_FILE:+COLOPHON_BACKUP_PASSPHRASE_FILE=}$key_file"
  fi
}

# encrypt <out>: encrypt stdin to <out>. The passphrase stays in its file — nothing on the
# command line, so `ps` never sees it.
encrypt() {
  if [[ "$ENC" == age ]]; then
    age -e -R "$(age-keygen -y "$key_file")" -o "$1"
  else
    gpg --batch --yes --pinentry-mode loopback --passphrase-file "$key_file" \
      --cipher-algo AES256 --symmetric --output "$1"
  fi
}

# decrypt <file>: plaintext on stdout.
decrypt() {
  if [[ "$ENC" == age ]]; then
    age -d -i "$key_file" "$1"
  else
    gpg --batch --yes --pinentry-mode loopback --passphrase-file "$key_file" --decrypt "$1"
  fi
}

# machine_name: the perf/results convention — "apple-m2-pro-12c", "amd-ryzen-9-5900x-12c", …
machine_name() {
  local cpu cores
  if [[ "$(uname -s)" == "Darwin" ]]; then
    cpu=$(sysctl -n machdep.cpu.brand_string); cores=$(sysctl -n hw.ncpu)
  else
    cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//'); cores=$(nproc)
  fi
  printf '%s-%sc' "$cpu" "$cores" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/-$//'
}
