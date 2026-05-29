# Sudoku solver via recursive backtracking on a 9x9 grid.
# Input: 9 lines of 9 characters. Empty cells are '.' or '0'.
# Output: 9 solved lines, then "STATUS: SOLVED" — or "STATUS: NO SOLUTION".
# Grid stored 1-indexed: g[r,c].

function place(r, c, v)   { g[r, c] = v; rowused[r, v] = 1; colused[c, v] = 1; boxused[bx(r, c), v] = 1 }
function unplace(r, c, v) { g[r, c] = 0; delete rowused[r, v]; delete colused[c, v]; delete boxused[bx(r, c), v] }
function bx(r, c) { return int((r - 1) / 3) * 3 + int((c - 1) / 3) + 1 }

function find_blank(   r, c) {
  for (r = 1; r <= 9; r++) {
    for (c = 1; c <= 9; c++) {
      if (g[r, c] == 0) {
        empty_r = r; empty_c = c; return 1
      }
    }
  }
  return 0
}

function solve(   r, c, v) {
  if (!find_blank()) return 1
  r = empty_r; c = empty_c
  for (v = 1; v <= 9; v++) {
    if ((r SUBSEP v) in rowused) continue
    if ((c SUBSEP v) in colused) continue
    if ((bx(r, c) SUBSEP v) in boxused) continue
    place(r, c, v)
    if (solve()) return 1
    unplace(r, c, v)
  }
  return 0
}

NR <= 9 {
  for (c = 1; c <= 9; c++) {
    ch = substr($0, c, 1)
    if (ch == "." || ch == "0" || ch == " ") g[NR, c] = 0
    else {
      v = ch + 0
      g[NR, c] = v
      rowused[NR, v] = 1
      colused[c, v] = 1
      boxused[bx(NR, c), v] = 1
    }
  }
}

END {
  ok = solve()
  for (r = 1; r <= 9; r++) {
    line = ""
    for (c = 1; c <= 9; c++) line = line g[r, c]
    print line
  }
  print ok ? "STATUS: SOLVED" : "STATUS: NO SOLUTION"
}
