# Merge overlapping closed intervals.
# Input lines: "<start> <end>"  (integers; start <= end).
# Output: merged intervals one per line, then "TOTAL: <covered_length>".
# Sort key padded to fixed width so awk's string asort gives numeric order
# regardless of magnitude.

NF == 2 {
  ni++
  s[ni] = $1 + 0; e[ni] = $2 + 0
}

END {
  for (i = 1; i <= ni; i++) keys[i] = sprintf("%012d %012d %d", s[i], e[i], i)
  asort(keys, sorted)

  cs = 0; ce = 0; have = 0; covered = 0
  for (i = 1; i <= ni; i++) {
    split(sorted[i], parts, " ")
    idx = parts[3] + 0
    ts = s[idx]; te = e[idx]
    if (!have) { cs = ts; ce = te; have = 1; continue }
    if (ts <= ce + 1) {     # overlap or adjacent
      if (te > ce) ce = te
    } else {
      printf "[%d, %d]\n", cs, ce
      covered += ce - cs + 1
      cs = ts; ce = te
    }
  }
  if (have) {
    printf "[%d, %d]\n", cs, ce
    covered += ce - cs + 1
  }
  printf "TOTAL: %d\n", covered
}
