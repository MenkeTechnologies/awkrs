# CSV pivot table: group rows by column $2 (host), sum column $3 (cpu_sec),
# count rows per host, and emit min / max / mean alongside.
# Input has a header row; we use -k (CSV mode) so the user invokes this with
# awkrs/gawk in CSV-aware mode; for parity here we use FS="," and assume no
# embedded commas (the .in file complies).
#
# Output is sorted by host name ascending for deterministic byte parity.

BEGIN { FS = "," }

NR == 1 { next }

{
  h = $2; v = $3 + 0; mem = $4 + 0
  cnt[h]++
  sum_cpu[h] += v
  sum_mem[h] += mem
  if (!(h in min_cpu) || v < min_cpu[h]) min_cpu[h] = v
  if (!(h in max_cpu) || v > max_cpu[h]) max_cpu[h] = v
}

END {
  printf "%-10s %5s %8s %8s %8s %10s\n", "host", "n", "min_cpu", "max_cpu", "avg_cpu", "tot_mem"
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (h in cnt) {
    printf "%-10s %5d %8.2f %8.2f %8.2f %10d\n", \
      h, cnt[h], min_cpu[h], max_cpu[h], sum_cpu[h] / cnt[h], sum_mem[h]
  }
}
