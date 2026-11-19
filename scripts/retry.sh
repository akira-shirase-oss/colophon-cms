#!/usr/bin/env bash
# Runs a command up to N times, waiting between tries (RETRY_DELAY seconds, default 30, doubling),
# and exits with the last try's status. CI wraps network installs in it (Playwright's system
# packages come from Ubuntu's mirrors, which drop connections now and then); never a test.
#
#   scripts/retry.sh <tries> <command> [args...]
set -uo pipefail

tries=${1:?usage: scripts/retry.sh <tries> <command> [args...]}
shift
[[ $tries =~ ^[1-9][0-9]*$ ]] || { echo "retry.sh: tries must be a positive integer, got '$tries'" >&2; exit 2; }
(($# > 0)) || { echo "usage: scripts/retry.sh <tries> <command> [args...]" >&2; exit 2; }

delay=${RETRY_DELAY:-30}
for ((attempt = 1; ; attempt++)); do
	"$@"
	status=$?
	((status == 0)) && exit 0
	if ((attempt >= tries)); then
		echo "retry.sh: '$1' failed $attempt times (last status $status)" >&2
		exit "$status"
	fi
	echo "retry.sh: '$1' failed (status $status), try $((attempt + 1)) of $tries in ${delay}s" >&2
	sleep "$delay"
	delay=$((delay * 2))
done
