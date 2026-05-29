# Prim's algorithm — minimum spanning tree via lazy linear scan of the
# frontier (simple, no PQ). The graph is undirected.
# Input lines: "<u> <v> <w>"   undirected edge u <-> v with weight w.
# Output: chosen edges in pick order, then "TOTAL: <sum>".
# Start vertex = lex-min node so output is deterministic.

function add_edge(u, v, w,   k) {
  if (!(u in seen)) { seen[u] = 1; outdeg[u] = 0 }
  if (!(v in seen)) { seen[v] = 1; outdeg[v] = 0 }
  k = ++outdeg[u]; adj_to[u, k] = v; adj_w[u, k] = w + 0
  k = ++outdeg[v]; adj_to[v, k] = u; adj_w[v, k] = w + 0
}

NF == 3 { add_edge($1, $2, $3) }

END {
  PROCINFO["sorted_in"] = "@ind_str_asc"
  start = ""
  for (n in seen) { start = n; break }
  if (start == "") { print "TOTAL: 0"; exit 0 }

  in_tree[start] = 1
  total = 0

  while (1) {
    best_w = -1; best_u = ""; best_v = ""
    for (u in in_tree) {
      for (k = 1; k <= outdeg[u]; k++) {
        v = adj_to[u, k]
        if (v in in_tree) continue
        w = adj_w[u, k]
        # Tie-break by (w asc, u asc, v asc) for deterministic output.
        if (best_w == -1 \
            || w < best_w \
            || (w == best_w && u < best_u) \
            || (w == best_w && u == best_u && v < best_v)) {
          best_w = w; best_u = u; best_v = v
        }
      }
    }
    if (best_w == -1) break
    printf "%s -- %s  w=%d\n", best_u, best_v, best_w
    total += best_w
    in_tree[best_v] = 1
  }
  printf "TOTAL: %d\n", total
}
