# Bellman-Ford single-source shortest paths with negative-weight edges.
# Input first line:  "SRC <node>"
# Subsequent lines:  "<u> <v> <w>"    directed edge u -> v, weight w (can be < 0).
# Output: for every reachable node (sorted lex):  "<node>  dist=<d>"
# Then "NEG CYCLE" if a negative-weight cycle is reachable, else "OK".

function add_edge(u, v, w) {
  nodes[u] = 1; nodes[v] = 1
  ne++
  eu[ne] = u; ev[ne] = v; ew[ne] = w + 0
}

NR == 1 && $1 == "SRC" { src = $2; next }
NF == 3                { add_edge($1, $2, $3) }

END {
  INF = 1000000000
  nn = 0
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (n in nodes) { nn++; order[nn] = n; dist[n] = INF }
  if (!(src in dist)) { print "no source"; exit 1 }
  dist[src] = 0

  for (pass = 1; pass < nn; pass++) {
    relaxed = 0
    for (i = 1; i <= ne; i++) {
      u = eu[i]; v = ev[i]; w = ew[i]
      if (dist[u] == INF) continue
      cand = dist[u] + w
      if (cand < dist[v]) { dist[v] = cand; relaxed = 1 }
    }
    if (!relaxed) break
  }

  neg = 0
  for (i = 1; i <= ne; i++) {
    u = eu[i]; v = ev[i]; w = ew[i]
    if (dist[u] != INF && dist[u] + w < dist[v]) { neg = 1; break }
  }

  for (k = 1; k <= nn; k++) {
    n = order[k]
    if (dist[n] != INF) printf "%s  dist=%d\n", n, dist[n]
  }
  print neg ? "NEG CYCLE" : "OK"
}
