# Kruskal's minimum spanning tree via union-find (path compression + union-by-rank).
# Input: each line is "<u> <v> <w>"   undirected edge with weight w.
# Output: chosen edges in pick order, then "TOTAL: <sum>".
# Ties broken by (weight asc, u asc, v asc) for deterministic byte parity.

function find(x) {
  while (parent[x] != x) {
    parent[x] = parent[parent[x]]   # path compression (halving)
    x = parent[x]
  }
  return x
}

function unite(a, b,   ra, rb) {
  ra = find(a); rb = find(b)
  if (ra == rb) return 0
  if (rank[ra] < rank[rb]) { parent[ra] = rb }
  else if (rank[ra] > rank[rb]) { parent[rb] = ra }
  else { parent[rb] = ra; rank[ra]++ }
  return 1
}

NF == 3 {
  ne++
  eu[ne] = $1; ev[ne] = $2; ew[ne] = $3 + 0
  if (!($1 in parent)) { parent[$1] = $1; rank[$1] = 0; nodes++ }
  if (!($2 in parent)) { parent[$2] = $2; rank[$2] = 0; nodes++ }
}

END {
  # Build sort key for each edge: weight (zero-padded), then u, then v.
  for (i = 1; i <= ne; i++) keys[i] = sprintf("%012d %s %s %d", ew[i], eu[i], ev[i], i)
  asort(keys, sorted)
  total = 0; picked = 0
  for (i = 1; i <= ne; i++) {
    split(sorted[i], parts, " ")
    idx = parts[4] + 0
    u = eu[idx]; v = ev[idx]; w = ew[idx]
    if (unite(u, v)) {
      printf "%s -- %s  w=%d\n", u, v, w
      total += w; picked++
      if (picked == nodes - 1) break
    }
  }
  printf "TOTAL: %d\n", total
}
