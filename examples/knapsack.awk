# 0/1 knapsack with traceback to recover the chosen items.
# Input first line:  "CAP <W>"      knapsack capacity
# Subsequent lines:  "<name> <weight> <value>"
# Output: chosen items (in input order), then "VALUE: <v>  WEIGHT: <w> / <cap>".
# DP table dp[i, w] is held in a SUBSEP 2D array.

NR == 1 && $1 == "CAP" { CAP = $2 + 0; next }
NF == 3 {
  n++
  nm[n] = $1; wt[n] = $2 + 0; vl[n] = $3 + 0
}

END {
  for (w = 0; w <= CAP; w++) dp[0, w] = 0
  for (i = 1; i <= n; i++) {
    for (w = 0; w <= CAP; w++) {
      dp[i, w] = dp[i - 1, w]
      if (wt[i] <= w) {
        cand = dp[i - 1, w - wt[i]] + vl[i]
        if (cand > dp[i, w]) dp[i, w] = cand
      }
    }
  }

  # Traceback.
  w = CAP
  for (i = n; i >= 1; i--) {
    if (dp[i, w] != dp[i - 1, w]) {
      taken[i] = 1
      w -= wt[i]
    }
  }

  used_w = 0
  for (i = 1; i <= n; i++) {
    if (taken[i]) {
      printf "  %s  w=%d v=%d\n", nm[i], wt[i], vl[i]
      used_w += wt[i]
    }
  }
  printf "VALUE: %d  WEIGHT: %d / %d\n", dp[n, CAP], used_w, CAP
}
