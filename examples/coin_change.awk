# Coin change: minimum number of coins to make a target amount.
# Each coin may be used any number of times (unbounded).
# DP table dp[s] = min coins for sum s (large sentinel for unreachable).
# Reconstruct by walking the predecessor table.
#
# Input first line: "AMOUNT <a>"
# Subsequent lines: one coin denomination per line.
# Output: "AMOUNT: <a>"
#         "COINS: c1+c2+... = <count>"
#         or "AMOUNT: <a>"
#            "IMPOSSIBLE"

NR == 1 && $1 == "AMOUNT" { A = $2 + 0; next }
{ nc++; cn[nc] = $1 + 0 }

END {
  INF = 1000000
  dp[0] = 0
  for (s = 1; s <= A; s++) dp[s] = INF
  for (s = 1; s <= A; s++) {
    for (i = 1; i <= nc; i++) {
      c = cn[i]
      if (c <= s && dp[s - c] + 1 < dp[s]) {
        dp[s] = dp[s - c] + 1
        prev[s] = c
      }
    }
  }

  printf "AMOUNT: %d\n", A
  if (dp[A] >= INF) { print "IMPOSSIBLE"; exit 0 }

  # Walk back.
  s = A
  while (s > 0) {
    chosen[++ncoins] = prev[s]
    s -= prev[s]
  }

  # Sort chosen ascending for a deterministic display.
  for (i = 2; i <= ncoins; i++) {
    k = chosen[i]; j = i - 1
    while (j >= 1 && chosen[j] > k) { chosen[j + 1] = chosen[j]; j-- }
    chosen[j + 1] = k
  }

  out = ""
  for (i = 1; i <= ncoins; i++) out = out (i == 1 ? "" : "+") chosen[i]
  printf "COINS: %s = %d\n", out, ncoins
}
