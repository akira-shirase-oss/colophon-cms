#!/usr/bin/env bash
# Installs the repository's git hooks (D-241): points this clone's core.hooksPath at .githooks/, so
# every worktree of it runs scripts/pre-commit.sh and scripts/pre-push.sh. Idempotent: running it
# again changes nothing.
#
#   scripts/install-hooks.sh                         the hooks
#   scripts/install-hooks.sh --require-email <addr>  and the pre-commit hook refuses a commit whose
#                                                    author or committer isn't <addr> (the
#                                                    project's builders; see AGENTS.md)
#   scripts/install-hooks.sh --uninstall             back to git's own hooks directory
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
case "${1:-}" in
  --uninstall)
    git config --unset core.hooksPath || true
    git config --unset colophon.requireEmail || true
    echo "hooks uninstalled"
    exit 0
    ;;
  --require-email)
    [[ -n "${2:-}" ]] || { echo "usage: scripts/install-hooks.sh --require-email <address>" >&2; exit 2; }
    git config colophon.requireEmail "$2"
    ;;
  "") ;;
  *)
    echo "usage: scripts/install-hooks.sh [--require-email <address> | --uninstall]" >&2
    exit 2
    ;;
esac
chmod +x .githooks/pre-commit .githooks/pre-push scripts/pre-commit.sh scripts/pre-push.sh
git config core.hooksPath .githooks
echo "hooks installed: core.hooksPath = .githooks$(
  required=$(git config --get colophon.requireEmail || true)
  [[ -n "$required" ]] && printf '; commits must be authored and committed as <%s>' "$required"
)"
