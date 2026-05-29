# Floyd-Warshall all-pairs shortest paths.
# Input lines:  "<u> <v> <w>"    directed edge u -> v with weight w (can be negative).
# Output: NxN matrix of pairwise distances (sorted by node name), with "INF"
# for unreachable pairs. Then "NEG CYCLE" if any node reaches itself via a
# negative-cost cycle, else "OK".
#
# Distance held in d[u, v] (SUBSEP 2D). Missing entries treated as INF.

function dist(u, v) { return ((u SUBSEP v) in d) ? d[u, v] : INF }
function setd(u, v, w) { d[u, v] = w }

NF == 3 {
  u = $1; v = $2; w = $3 + 0
  nodes[u] = 1; nodes[v] = 1
  if (!((u SUBSEP v) in d) || w < d[u, v]) d[u, v] = w
}

END {
  INF = 1000000000

  for (u in nodes) setd(u, u, 0)

  # Stable order via sorted node list.
  n = 0
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (u in nodes) { n++; order[n] = u }

  for (k = 1; k <= n; k++) {
    K = order[k]
    for (i = 1; i <= n; i++) {
      I = order[i]
      if (dist(I, K) == INF) continue
      for (j = 1; j <= n; j++) {
        J = order[j]
        if (dist(K, J) == INF) continue
        cand = dist(I, K) + dist(K, J)
        if (cand < dist(I, J)) setd(I, J, cand)
      }
    }
  }

  # Header.
  printf "%-6s", ""
  for (j = 1; j <= n; j++) printf "%8s", order[j]
  print ""
  for (i = 1; i <= n; i++) {
    printf "%-6s", order[i]
    for (j = 1; j <= n; j++) {
      v = dist(order[i], order[j])
      if (v == INF) printf "%8s", "INF"
      else          printf "%8d", v
    }
    print ""
  }

  neg = 0
  for (i = 1; i <= n; i++) if (dist(order[i], order[i]) < 0) { neg = 1; break }
  print neg ? "NEG CYCLE" : "OK"
}
