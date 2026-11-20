#!/usr/bin/env bash
# D-223: adds latency between the server and Postgres, and only there. The quick start's services share one network
# namespace (its `pod` container), so the server reaches Postgres over loopback: a netem delay on `lo`, filtered to
# TCP port 5432 in both directions, adds <ms> to each way of every server-Postgres packet (so 2 ms makes every round
# trip 4 ms slower), and leaves Valkey, the object store and the k6 traffic alone.
#
#   perf/saturation/netem.sh <pod container> <ms>   (0 removes it)
#
# Prints {"delay_ms": N, "connect_us_5432": median of 20 TCP connects, "connect_us_6379": the same to Valkey}, so
# the record carries the latency measured, not only the one asked for.
set -euo pipefail
pod=$1 ms=$2
[[ $ms =~ ^[0-9]+$ ]] || { echo "netem.sh: <ms> must be a whole number" >&2; exit 2; }
image=alpine:3.23@sha256:469c9ea410b9215eaee7976ba6c0639039b444be8e5d638f7abbada189971eb2
docker run --rm --network "container:$pod" --cap-add NET_ADMIN -e MS="$ms" "$image" sh -euc '
  apk add -q --no-cache iproute2-tc coreutils >/dev/null
  tc qdisc del dev lo root 2>/dev/null || true
  if [ "$MS" -gt 0 ]; then
    tc qdisc add dev lo root handle 1: prio bands 4
    tc qdisc add dev lo parent 1:4 handle 40: netem delay "${MS}ms"
    tc filter add dev lo parent 1:0 protocol ip u32 match ip dport 5432 0xffff flowid 1:4
    tc filter add dev lo parent 1:0 protocol ip u32 match ip sport 5432 0xffff flowid 1:4
  fi
  connect() {
    for i in $(seq 20); do s=$(date +%s%N); nc -z -w1 127.0.0.1 "$1"; e=$(date +%s%N); echo $(( (e - s) / 1000 )); done \
      | sort -n | awk "{v[NR] = \$1} END {print v[int((NR + 1) / 2)]}"
  }
  printf "{\"delay_ms\":%s,\"connect_us_5432\":%s,\"connect_us_6379\":%s}\n" "$MS" "$(connect 5432)" "$(connect 6379)"
'
