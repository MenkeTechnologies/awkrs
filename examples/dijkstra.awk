# Dijkstra single-source shortest paths on a directed weighted graph.
# Input first line:  "SRC <node>"
# Subsequent lines:  "<u> <v> <w>"   directed edge u -> v with weight w >= 0.
# Output: for every reachable node (sorted lex):
#   "<node>  dist=<d>  path: src -> ... -> node"
#
# Priority queue is a (key, value) min-heap keyed on distance; ties broken by
# node name so output is deterministic when several frontier nodes share dist.
# Lazy deletion: when we pop a stale entry (curd > dist[u]) we just skip it.

function hpush(d, n,   i, td, tn, p) {
  hn++
  hd[hn] = d; hk[hn] = n
  i = hn
  while (i > 1) {
    p = int(i / 2)
    if (hd[p] > hd[i] || (hd[p] == hd[i] && hk[p] > hk[i])) {
      td = hd[p]; tn = hk[p]
      hd[p] = hd[i]; hk[p] = hk[i]
      hd[i] = td;    hk[i] = tn
      i = p
    } else { break }
  }
}

function hpop(   rd, rn, i, l, r, sm, td, tn) {
  rd = hd[1]; rn = hk[1]
  hd[1] = hd[hn]; hk[1] = hk[hn]
  delete hd[hn]; delete hk[hn]
  hn--
  i = 1
  while (1) {
    l = 2 * i; r = l + 1; sm = i
    if (l <= hn && (hd[l] < hd[sm] || (hd[l] == hd[sm] && hk[l] < hk[sm]))) sm = l
    if (r <= hn && (hd[r] < hd[sm] || (hd[r] == hd[sm] && hk[r] < hk[sm]))) sm = r
    if (sm == i) break
    td = hd[i]; tn = hk[i]
    hd[i] = hd[sm]; hk[i] = hk[sm]
    hd[sm] = td;    hk[sm] = tn
    i = sm
  }
  popd = rd; popk = rn
}

function add_edge(u, v, w,   k) {
  if (!(u in seen)) { seen[u] = 1; outdeg[u] = 0 }
  if (!(v in seen)) { seen[v] = 1; outdeg[v] = 0 }
  k = ++outdeg[u]
  adj_to[u, k] = v
  adj_w[u, k]  = w + 0
}

NR == 1 && $1 == "SRC" { src = $2; next }
NF == 3                { add_edge($1, $2, $3) }

END {
  if (src == "") { print "no source"; exit 1 }
  hn = 0
  dist[src] = 0; parent[src] = ""
  hpush(0, src)
  while (hn > 0) {
    hpop()
    u = popk; curd = popd
    if (curd > dist[u]) continue   # stale entry
    for (k = 1; k <= outdeg[u]; k++) {
      v = adj_to[u, k]; w = adj_w[u, k]
      nd = curd + w
      if (!(v in dist) || nd < dist[v]) {
        dist[v] = nd; parent[v] = u
        hpush(nd, v)
      }
    }
  }
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (n in dist) {
    path = n; p = parent[n]
    while (p != "") { path = p " -> " path; p = parent[p] }
    printf "%s  dist=%d  path: %s\n", n, dist[n], path
  }
}
