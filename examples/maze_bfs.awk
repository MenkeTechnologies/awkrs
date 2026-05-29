# BFS through an ASCII maze.
# Input:  a rectangular grid of '#' (wall), '.' (open), 'S' (start), 'E' (end).
# Output: the maze with the shortest S->E path marked '*'; followed by
#         "STEPS: <n>"  or  "STEPS: NO PATH".
# 4-neighbour movement. Ties broken by neighbour order (up, right, down, left)
# for deterministic byte parity.

function in_bounds(r, c) { return (r >= 1 && r <= R && c >= 1 && c <= C) }
function key(r, c)       { return r SUBSEP c }

{
  rows++
  grid[rows] = $0
  if (length($0) > C) C = length($0)
  for (c = 1; c <= length($0); c++) {
    ch = substr($0, c, 1)
    if (ch == "S") { sr = rows; sc = c }
    if (ch == "E") { er = rows; ec = c }
  }
}

END {
  R = rows
  # 4-neighbour deltas: up, right, down, left.
  dr[1] = -1; dc[1] = 0
  dr[2] = 0;  dc[2] = 1
  dr[3] = 1;  dc[3] = 0
  dr[4] = 0;  dc[4] = -1

  qh = 1; qt = 0
  qt++; qr[qt] = sr; qc[qt] = sc
  dist[key(sr, sc)] = 0
  parent[key(sr, sc)] = ""

  while (qh <= qt) {
    r = qr[qh]; c = qc[qh]; qh++
    if (r == er && c == ec) break
    for (k = 1; k <= 4; k++) {
      nr = r + dr[k]; nc = c + dc[k]
      if (!in_bounds(nr, nc)) continue
      ch = substr(grid[nr], nc, 1)
      if (ch == "#") continue
      if (key(nr, nc) in dist) continue
      dist[key(nr, nc)] = dist[key(r, c)] + 1
      parent[key(nr, nc)] = r SUBSEP c
      qt++; qr[qt] = nr; qc[qt] = nc
    }
  }

  if (!(key(er, ec) in dist)) {
    for (i = 1; i <= R; i++) print grid[i]
    print "STEPS: NO PATH"
    exit 0
  }

  # Walk parent chain, marking path cells (excluding S and E).
  cur = key(er, ec)
  while (cur != "") {
    split(cur, rc, SUBSEP)
    r = rc[1]; c = rc[2]
    on_path[r SUBSEP c] = 1
    cur = parent[cur]
  }

  for (i = 1; i <= R; i++) {
    line = ""
    L = length(grid[i])
    for (c = 1; c <= L; c++) {
      ch = substr(grid[i], c, 1)
      if (ch == "S" || ch == "E") { line = line ch; continue }
      if ((i SUBSEP c) in on_path) { line = line "*"; continue }
      line = line ch
    }
    print line
  }
  printf "STEPS: %d\n", dist[key(er, ec)]
}
