# perf-compare's reader of admission_throughput.rs's output: one JSON object per "N=..." line, with
# the round, side and mode it was given (-v). Used by scripts/perf-compare.sh, tested by
# scripts/test-admission-lines.sh.
# A level's line may follow libtest's "test <name> ... " on the same line (the first one does).
match($0, /N= *[0-9]+ /) {
  $0 = substr($0, RSTART)
  gsub(/= +/, "=")
  line = "{\"round\":" round ",\"side\":\"" side "\",\"mode\":\"" mode "\""
  for (i = 1; i <= NF; i++) { split($i, kv, "="); line = line ",\"" kv[1] "\":" kv[2] }
  print line "}"; fflush()
}
