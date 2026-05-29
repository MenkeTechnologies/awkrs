# N-queens backtracking solver.
# Input: a single line with N (the board size).
# Output: every valid placement as N lines of '.' and 'Q', separated by a
# blank line; then "SOLUTIONS: <count>".
# Pruning uses three constant-time witnesses:
#   col[c]      → column c already occupied
#   diag1[r+c]  → "\" diagonal (r+c is invariant along it)
#   diag2[r-c]  → "/" diagonal (r-c is invariant along it)

function solve(row,   c) {
  if (row > N) {
    sols++
    for (r = 1; r <= N; r++) {
      line = ""
      for (c = 1; c <= N; c++) line = line ((board[r] == c) ? "Q" : ".")
      print line
    }
    print ""
    return
  }
  for (c = 1; c <= N; c++) {
    if ((c in col) || ((row + c) in diag1) || ((row - c) in diag2)) continue
    board[row] = c
    col[c] = 1; diag1[row + c] = 1; diag2[row - c] = 1
    solve(row + 1)
    delete col[c]; delete diag1[row + c]; delete diag2[row - c]
  }
}

NR == 1 {
  N = $1 + 0
  if (N < 1) { print "N must be >= 1"; exit 1 }
  solve(1)
  printf "SOLUTIONS: %d\n", sols
  exit 0
}
