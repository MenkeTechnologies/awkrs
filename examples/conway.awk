# Conway's Game of Life — fixed-grid evolution for N generations.
# Input first line:  "GEN <n> WIDTH <w> HEIGHT <h>"
# Lines 2..h+1:      grid rows (use '#' for live, '.' for dead; any width).
# Output: generation 0 (initial), then each subsequent generation through
# generation N, separated by blank lines, each labelled "-- gen <k> --".

function get(r, c) {
  if (r < 1 || r > H || c < 1 || c > W) return 0
  return (cell[r, c] == 1) ? 1 : 0
}

function neighbors(r, c) {
  return get(r-1,c-1)+get(r-1,c)+get(r-1,c+1)+ \
         get(r,c-1)+              get(r,c+1)+ \
         get(r+1,c-1)+get(r+1,c)+get(r+1,c+1)
}

function step() {
  delete nxt
  for (r = 1; r <= H; r++) {
    for (c = 1; c <= W; c++) {
      n = neighbors(r, c)
      alive = (cell[r, c] == 1)
      if (alive && (n == 2 || n == 3)) nxt[r, c] = 1
      else if (!alive && n == 3)       nxt[r, c] = 1
      else                             delete nxt[r, c]
    }
  }
  delete cell
  for (k in nxt) cell[k] = nxt[k]
}

function show(label,   r, c, line) {
  printf "-- %s --\n", label
  for (r = 1; r <= H; r++) {
    line = ""
    for (c = 1; c <= W; c++) line = line ((cell[r, c] == 1) ? "#" : ".")
    print line
  }
}

NR == 1 && $1 == "GEN" { N = $2 + 0; W = $4 + 0; H = $6 + 0; row = 0; next }
{
  row++
  for (c = 1; c <= length($0) && c <= W; c++) {
    if (substr($0, c, 1) == "#") cell[row, c] = 1
  }
}

END {
  show("gen 0")
  for (g = 1; g <= N; g++) {
    step()
    print ""
    show("gen " g)
  }
}
