# Kahn's algorithm topological sort with cycle detection.
# Input lines: "u v" meaning edge u -> v.
# Output:      space-separated topo order (ties broken by sorted node name);
#              or "CYCLE" if the graph has one.
#
# Data:
#   indeg[node]       in-degree counter
#   adj[u, k]         k-th successor of u (1..outdeg[u])
#   outdeg[u]         count of successors
#   nodes_seen[name]  any node mentioned, used for completeness check

function add_edge(u, v,   k) {
  if (!(u in nodes_seen)) { nodes_seen[u] = 1; indeg[u] = 0; outdeg[u] = 0 }
  if (!(v in nodes_seen)) { nodes_seen[v] = 1; indeg[v] = 0; outdeg[v] = 0 }
  k = ++outdeg[u]
  adj[u, k] = v
  indeg[v]++
}

NF == 2 { add_edge($1, $2) }

END {
  # ready queue = nodes with indeg 0, picked in sorted order for determinism.
  PROCINFO["sorted_in"] = "@ind_str_asc"
  out = ""; sep = ""
  emitted = 0; total = 0
  for (n in nodes_seen) total++

  # Re-scan ready set each round (small graphs, clarity over speed).
  while (1) {
    pick = ""
    for (n in nodes_seen) {
      if (!(n in done) && indeg[n] == 0) { pick = n; break }
    }
    if (pick == "") break
    out = out sep pick; sep = " "
    done[pick] = 1
    emitted++
    for (k = 1; k <= outdeg[pick]; k++) {
      v = adj[pick, k]
      indeg[v]--
    }
  }

  if (emitted == total) print out
  else print "CYCLE"
}
