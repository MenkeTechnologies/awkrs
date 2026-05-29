# Tarjan's strongly connected components.
# Input: each line "<u> <v>" is a directed edge u -> v.
# Output: each SCC on its own line as "{ a b c }" with members lex-sorted;
# components emitted in lex order of their smallest member, then "COUNT: <k>".
#
# Iterative DFS would obscure the algorithm — use recursion with explicit
# stack[], onstack[], index[], lowlink[] state.

function add_edge(u, v,   k) {
  if (!(u in seen)) seen[u] = 1
  if (!(v in seen)) seen[v] = 1
  k = ++outdeg[u]
  adj[u, k] = v
}

function strongconnect(v,   k, w, smallest, members, line, n, arr, i, j, key, m) {
  idx[v] = idx_counter
  low[v] = idx_counter
  idx_counter++
  stack[++sp] = v
  onstack[v] = 1

  for (k = 1; k <= (outdeg[v] + 0); k++) {
    w = adj[v, k]
    if (!(w in idx)) {
      strongconnect(w)
      if (low[w] < low[v]) low[v] = low[w]
    } else if (w in onstack) {
      if (idx[w] < low[v]) low[v] = idx[w]
    }
  }

  if (low[v] == idx[v]) {
    # Pop SCC.
    delete members
    smallest = ""
    while (1) {
      w = stack[sp]; delete stack[sp]; sp--
      delete onstack[w]
      members[w] = 1
      if (smallest == "" || w < smallest) smallest = w
      if (w == v) break
    }
    # Build "{ a b c }" with members in lex order via insertion sort.
    # (awkrs `asort` on a function-local array is currently buggy — sees only
    # the global by that name — so we hand-sort here to keep the example
    # self-contained.)
    delete arr; n = 0
    for (m in members) { n++; arr[n] = m }
    for (i = 2; i <= n; i++) {
      key = arr[i]; j = i - 1
      while (j >= 1 && arr[j] > key) { arr[j + 1] = arr[j]; j-- }
      arr[j + 1] = key
    }
    line = "{"
    for (i = 1; i <= n; i++) line = line " " arr[i]
    line = line " }"
    scc_keys[smallest] = line
  }
}

NF == 2 { add_edge($1, $2) }

END {
  idx_counter = 0
  sp = 0
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (v in seen) if (!(v in idx)) strongconnect(v)
  for (k in scc_keys) { print scc_keys[k]; count++ }
  printf "COUNT: %d\n", count
}
