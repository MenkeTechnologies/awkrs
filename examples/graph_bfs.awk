# Breadth-first search on an undirected unweighted graph.
# Input first line:  "SRC <node>"      starting node for the search
# Subsequent lines:  "<u> <v>"        undirected edge u <-> v
# Output: for every reachable node (sorted lex), "<node> <dist> <path>".
#
# Adjacency stored as adj[u, k] (k-th neighbor), outdeg[u].
# Queue implemented as q[1..qe], qh = head index.

function add_edge(u, v,   k) {
  if (!(u in seen)) { seen[u] = 1; outdeg[u] = 0 }
  if (!(v in seen)) { seen[v] = 1; outdeg[v] = 0 }
  k = ++outdeg[u]; adj[u, k] = v
  k = ++outdeg[v]; adj[v, k] = u
}

NR == 1 && $1 == "SRC" { src = $2; next }
NF == 2                { add_edge($1, $2) }

END {
  if (src == "") { print "no source"; exit 1 }
  if (!(src in seen)) { printf "%s 0 %s\n", src, src; exit 0 }

  dist[src] = 0; parent[src] = ""
  qh = 1; qe = 0
  q[++qe] = src
  while (qh <= qe) {
    u = q[qh++]
    for (k = 1; k <= outdeg[u]; k++) {
      v = adj[u, k]
      if (v in dist) continue
      dist[v] = dist[u] + 1
      parent[v] = u
      q[++qe] = v
    }
  }

  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (n in dist) {
    # Reconstruct path src -> n.
    path = n; p = parent[n]
    while (p != "") { path = p " -> " path; p = parent[p] }
    printf "%s %d %s\n", n, dist[n], path
  }
}
