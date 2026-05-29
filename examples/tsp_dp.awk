# Held-Karp bitmask DP for Travelling Salesman (smallest-tour).
# Input first line:  "N <n>"   number of cities, 1..N (1 is the start/return).
# Subsequent N lines: each is "<d_1> <d_2> ... <d_N>" — the distance matrix
#                     (symmetric, d[i,i] = 0).
# Output:
#   "TOUR: 1 -> ... -> 1"
#   "COST: <c>"
# State dp[mask, last] = best cost reaching `last` having visited cities in
# `mask`, starting from city 1 (mask always includes bit 0).
#
# N is bounded by 15 because we represent visited-cities as a bitmask up to
# 2^N entries; gawk's `and`/`or`/`lshift` need non-negative operands.

function bit(i) { return lshift(1, i - 1) }    # 1-based city -> bit position
function has_bit(mask, i) { return and(mask, bit(i)) != 0 }
function set_bit(mask, i) { return or(mask, bit(i)) }

NR == 1 && $1 == "N" { N = $2 + 0; next }
{
  row++
  for (c = 1; c <= NF; c++) d[row, c] = $c + 0
}

END {
  if (N < 2 || N > 15) { print "N out of range (2..15)"; exit 1 }

  full = lshift(1, N) - 1

  # Init: from start (city 1), having visited only city 1.
  dp[bit(1), 1] = 0

  for (mask = 1; mask <= full; mask += 2) {   # mask must always include bit 0
    for (last = 1; last <= N; last++) {
      if (!has_bit(mask, last)) continue
      if (!((mask SUBSEP last) in dp)) continue
      base = dp[mask, last]
      for (nxt = 2; nxt <= N; nxt++) {
        if (has_bit(mask, nxt)) continue
        new_mask = set_bit(mask, nxt)
        cand = base + d[last, nxt]
        if (!((new_mask SUBSEP nxt) in dp) || cand < dp[new_mask, nxt]) {
          dp[new_mask, nxt] = cand
          par[new_mask, nxt] = last
        }
      }
    }
  }

  best = -1; best_last = 0
  for (last = 2; last <= N; last++) {
    if (!((full SUBSEP last) in dp)) continue
    cand = dp[full, last] + d[last, 1]
    if (best == -1 || cand < best) { best = cand; best_last = last }
  }
  if (best == -1) { print "NO TOUR"; exit 1 }

  # Reconstruct.
  path[1] = 1
  cur = best_last; mask = full
  k = 1
  while (cur != 1) {
    k++
    rev[k] = cur
    p = par[mask, cur]
    mask = and(mask, lshift(1, N) - 1 - bit(cur))   # clear bit `cur`
    cur = p
  }
  # Build forward order.
  pi = 0
  pi++; path[pi] = 1
  for (i = k; i >= 2; i--) { pi++; path[pi] = rev[i] }
  pi++; path[pi] = 1

  out = path[1]
  for (i = 2; i <= pi; i++) out = out " -> " path[i]
  printf "TOUR: %s\n", out
  printf "COST: %d\n", best
}
