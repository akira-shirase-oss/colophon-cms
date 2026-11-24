#!/usr/bin/env bash
# Shared by scripts/pre-commit.sh and scripts/pre-push.sh (D-241): timed steps with one line of
# progress each, the logged escape hatch, the toolchain the repository pins, and the leak check.
# Sourced, never run.

hook_name=${hook_name:-hook}
hook_started=$(date +%s)
hook_failed=""

# The toolchain rust-toolchain.toml pins, ahead of any other cargo on PATH (a Homebrew cargo
# ignores the pin), and two build jobs: hooks run on laptops other sessions share.
hook_toolchain() {
  if command -v rustup >/dev/null 2>&1; then
    local cargo
    if cargo=$(rustup which cargo 2>/dev/null); then
      PATH="$(dirname "$cargo"):$PATH"
      export PATH
    fi
  fi
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
  export SQLX_OFFLINE="${SQLX_OFFLINE:-true}"
  export CARGO_BUILD_WARNINGS=deny
}

# Git runs a hook with its own variables set (GIT_DIR, and GIT_INDEX_FILE for a commit's own index).
# A hook reads what it needs from git first, then clears them: a tool or test that runs git in
# another directory (colophon-version's stamp tests) must find that directory's repository, not this.
hook_clean_git_env() {
  unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_OBJECT_DIRECTORY \
    GIT_ALTERNATE_OBJECT_DIRECTORIES GIT_QUARANTINE_PATH GIT_EXEC_PATH GIT_REFLOG_ACTION
}

# COLOPHON_SKIP_HOOKS=1 skips the hook, for a work-in-progress commit during a mutation proof only;
# every skip is logged (COLOPHON_SKIP_REASON says why). `git commit --no-verify` also skips it but
# leaves no record, so the variable is the one to use.
hook_skip_if_asked() {
  [[ "${COLOPHON_SKIP_HOOKS:-0}" == "1" ]] || return 0
  local log
  log="$(git rev-parse --git-common-dir)/colophon-hooks-skipped.log"
  printf '%s\t%s\t%s\t%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$hook_name" \
    "$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')" \
    "$(git rev-parse --short HEAD 2>/dev/null || echo '-')" \
    "${COLOPHON_SKIP_REASON:-no reason given}" >>"$log"
  echo "$hook_name: SKIPPED (COLOPHON_SKIP_HOOKS=1; logged to $log)" >&2
  exit 0
}

# step <name> <command...>: one line when it starts, one when it ends, with its time; its output
# only when it fails. The first failure is the verdict; later steps still run, so one pass names
# every problem.
step() {
  local name=$1
  shift
  local started out status
  started=$(date +%s)
  printf '%s: %-34s ' "$hook_name" "$name" >&2
  out=$(mktemp)
  if "$@" >"$out" 2>&1; then
    status=0
  else
    status=$?
  fi
  local took=$(( $(date +%s) - started ))
  if (( status == 0 )); then
    printf 'ok   (%ss)\n' "$took" >&2
  else
    printf 'FAIL (%ss)\n' "$took" >&2
    sed 's/^/    /' "$out" | tail -n 60 >&2
    [[ -n "$hook_failed" ]] || hook_failed=$name
  fi
  rm -f "$out"
}

# The verdict line, and the hook's exit status.
hook_verdict() {
  local took=$(( $(date +%s) - hook_started ))
  if [[ -n "$hook_failed" ]]; then
    echo "$hook_name: FAILED at \"$hook_failed\" after ${took}s; fix it, or see AGENTS.md for the logged escape hatch" >&2
    exit 1
  fi
  echo "$hook_name: passed in ${took}s" >&2
}

# leak_check <diff-file>: no added line names this machine's owner, host or home directory. The
# patterns come from the machine itself (the global git name, the login, the host name), never from
# the repository, plus any regexes, one a line, in ~/.config/colophon/leak-patterns.
leak_check() {
  local diff=$1 patterns=() name login host extra
  patterns+=('/Users/[A-Za-z]' '/home/[a-z][a-z0-9_-]*/')
  name=$(git config --global user.name 2>/dev/null || true)
  [[ ${#name} -ge 4 && "$name" != akira-shirase-oss ]] && patterns+=("$name")
  login=$(id -un 2>/dev/null || true)
  [[ ${#login} -ge 4 && "$login" != runner && "$login" != root ]] && patterns+=("$login")
  host=$(hostname -s 2>/dev/null || true)
  [[ ${#host} -ge 4 && "$host" != localhost ]] && patterns+=("$host")
  extra="${XDG_CONFIG_HOME:-$HOME/.config}/colophon/leak-patterns"
  if [[ -f "$extra" ]]; then
    while IFS= read -r line; do
      [[ -n "$line" && "$line" != \#* ]] && patterns+=("$line")
    done <"$extra"
  fi
  local added hits=0 pattern found
  added=$(grep -E '^\+' "$diff" | grep -vE '^\+\+\+ ' || true)
  for pattern in "${patterns[@]}"; do
    # /home/runner/ is GitHub's runner, not a person.
    found=$(grep -iE -- "$pattern" <<<"$added" | grep -v '/home/runner/' || true)
    if [[ -n "$found" ]]; then
      head -3 <<<"$found" | sed 's/^/    leak: /'
      hits=$((hits + 1))
    fi
  done
  (( hits == 0 ))
}

# pushed_identity_check RANGE...: when this checkout asks for an identity, every commit being pushed
# that origin/main (as last fetched) doesn't already have must carry it as author and committer —
# so a commit some tool or test made in this repository under another name never leaves it.
pushed_identity_check() {
  local want range wrong=""
  want=$(git config --get colophon.requireEmail || true)
  [[ -n "$want" ]] || return 0
  for range in "$@"; do
    wrong+=$(git log --format='%h author <%ae> committer <%ce> %s' "$range" --not origin/main 2>/dev/null |
      awk -v want="$want" 'index($0, "author <" want "> committer <" want ">") == 0')
  done
  if [[ -n "$wrong" ]]; then
    echo "commits not by <$want> (author and committer both); fix them before pushing:"
    echo "$wrong"
    return 1
  fi
}

# identity_check: when this checkout asks for one (scripts/install-hooks.sh --require-email), the
# author and committer must both be that address. Builders of the project set it; contributors don't.
identity_check() {
  local want author committer
  want=$(git config --get colophon.requireEmail || true)
  [[ -n "$want" ]] || return 0
  author=$(git var GIT_AUTHOR_IDENT | sed -E 's/.*<([^>]*)>.*/\1/')
  committer=$(git var GIT_COMMITTER_IDENT | sed -E 's/.*<([^>]*)>.*/\1/')
  if [[ "$author" != "$want" || "$committer" != "$want" ]]; then
    echo "author <$author>, committer <$committer>: this checkout requires <$want> for both"
    return 1
  fi
}
