# Subset-sum DP: does any subset of the input integers sum to TARGET?
# Input first line: "TARGET <t>"
# Subsequent lines: one integer per line (non-negative).
# Output: "YES { a, b, c, ... }" with the chosen subset (preserving input
# order) or "NO".
# Standard DP on dp[i, s] = can prefix [1..i] sum to s? Reconstruct by
# walking the table.

NR == 1 && $1 == "TARGET" { T = $2 + 0; next }
{ n++; a[n] = $1 + 0 }

END {
  for (s = 0; s <= T; s++) dp[0, s] = (s == 0) ? 1 : 0
  for (i = 1; i <= n; i++) {
    for (s = 0; s <= T; s++) {
      dp[i, s] = dp[i - 1, s]
      if (s - a[i] >= 0 && dp[i - 1, s - a[i]]) dp[i, s] = 1
    }
  }

  if (!dp[n, T]) { print "NO"; exit 0 }

  # Reconstruct one valid subset.
  s = T
  for (i = n; i >= 1; i--) {
    if (s - a[i] >= 0 && dp[i - 1, s - a[i]]) {
      taken[i] = 1
      s -= a[i]
    }
  }

  out = "YES { "
  sep = ""
  for (i = 1; i <= n; i++) {
    if (taken[i]) { out = out sep a[i]; sep = ", " }
  }
  out = out " }"
  print out
}
