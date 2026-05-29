# A* shortest-path on an ASCII grid (4-neighbour moves) with Manhattan h.
# Input: a rectangular grid. '#' = wall, '.' = open, 'S' = start, 'E' = goal.
# Output: maze with path overlaid in '*', then "STEPS: <n>" (or "NO PATH").
# Open set is a min-heap keyed on (f, g, h, position) for deterministic tie
# breaks; closed set is a sparse SUBSEP array.

function abs1(x) { return (x < 0) ? -x : x }
function in_bounds(r, c) { return (r >= 1 && r <= R && c >= 1 && c <= C) }
function pkey(r, c) { return r SUBSEP c }

# Min-heap by (f, g, h, key) lex tuple.
function hpush(f, g, h, key,   i, p, tf, tg, th, tk) {
  hn++
  hf[hn] = f; hg[hn] = g; hh[hn] = h; hk[hn] = key
  i = hn
  while (i > 1) {
    p = int(i / 2)
    if (hf[p] > hf[i] \
        || (hf[p] == hf[i] && hg[p] > hg[i]) \
        || (hf[p] == hf[i] && hg[p] == hg[i] && hh[p] > hh[i]) \
        || (hf[p] == hf[i] && hg[p] == hg[i] && hh[p] == hh[i] && hk[p] > hk[i])) {
      tf = hf[p]; tg = hg[p]; th = hh[p]; tk = hk[p]
      hf[p] = hf[i]; hg[p] = hg[i]; hh[p] = hh[i]; hk[p] = hk[i]
      hf[i] = tf;    hg[i] = tg;    hh[i] = th;    hk[i] = tk
      i = p
    } else { break }
  }
}

function less(a, b) {
  if (hf[a] != hf[b]) return hf[a] < hf[b]
  if (hg[a] != hg[b]) return hg[a] < hg[b]
  if (hh[a] != hh[b]) return hh[a] < hh[b]
  return hk[a] < hk[b]
}

function hpop(   l, r, sm, tf, tg, th, tk, i) {
  pop_f = hf[1]; pop_g = hg[1]; pop_h = hh[1]; pop_k = hk[1]
  hf[1] = hf[hn]; hg[1] = hg[hn]; hh[1] = hh[hn]; hk[1] = hk[hn]
  delete hf[hn]; delete hg[hn]; delete hh[hn]; delete hk[hn]
  hn--
  i = 1
  while (1) {
    l = 2 * i; r = l + 1; sm = i
    if (l <= hn && less(l, sm)) sm = l
    if (r <= hn && less(r, sm)) sm = r
    if (sm == i) break
    tf = hf[i]; tg = hg[i]; th = hh[i]; tk = hk[i]
    hf[i] = hf[sm]; hg[i] = hg[sm]; hh[i] = hh[sm]; hk[i] = hk[sm]
    hf[sm] = tf;    hg[sm] = tg;    hh[sm] = th;    hk[sm] = tk
    i = sm
  }
}

{
  R++
  grid[R] = $0
  if (length($0) > C) C = length($0)
  for (c = 1; c <= length($0); c++) {
    ch = substr($0, c, 1)
    if (ch == "S") { sr = R; sc = c }
    if (ch == "E") { er = R; ec = c }
  }
}

END {
  dr[1] = -1; dc[1] = 0
  dr[2] = 0;  dc[2] = 1
  dr[3] = 1;  dc[3] = 0
  dr[4] = 0;  dc[4] = -1

  g_score[pkey(sr, sc)] = 0
  h0 = abs1(sr - er) + abs1(sc - ec)
  hn = 0
  hpush(h0, 0, h0, pkey(sr, sc))

  found = 0
  while (hn > 0) {
    hpop()
    cur = pop_k; curg = pop_g
    if (cur in closed) continue
    closed[cur] = 1
    split(cur, rc, SUBSEP); r = rc[1]; c = rc[2]
    if (r == er && c == ec) { found = 1; break }
    for (k = 1; k <= 4; k++) {
      nr = r + dr[k]; nc = c + dc[k]
      if (!in_bounds(nr, nc)) continue
      ch = substr(grid[nr], nc, 1)
      if (ch == "#") continue
      nk = pkey(nr, nc)
      tentative = curg + 1
      if ((nk in g_score) && tentative >= g_score[nk]) continue
      g_score[nk] = tentative
      parent[nk] = cur
      hnow = abs1(nr - er) + abs1(nc - ec)
      hpush(tentative + hnow, tentative, hnow, nk)
    }
  }

  if (!found) {
    for (i = 1; i <= R; i++) print grid[i]
    print "STEPS: NO PATH"
    exit 0
  }

  cur = pkey(er, ec)
  while (cur != "") {
    on_path[cur] = 1
    cur = parent[cur]
  }

  for (i = 1; i <= R; i++) {
    line = ""
    L = length(grid[i])
    for (c = 1; c <= L; c++) {
      ch = substr(grid[i], c, 1)
      if (ch == "S" || ch == "E") { line = line ch; continue }
      if (pkey(i, c) in on_path) { line = line "*"; continue }
      line = line ch
    }
    print line
  }
  printf "STEPS: %d\n", g_score[pkey(er, ec)]
}
