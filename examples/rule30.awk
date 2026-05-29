# Wolfram Rule 30 — elementary 1D cellular automaton. The cells live on a
# wrap-around tape of width W; the rule is the 8-bit lookup keyed on
# (left, center, right) triples.
#
# Input first line:  "W <w> GEN <g>"     tape width and generations to run
# Second line:       initial tape — w characters of '#' (live) and '.' (dead)
# Output: g+1 generations of width-w tape (gen 0 first), then "LIVE: <c1> <c2> ..."
# where ci is the live-cell count for generation i.

NR == 1 && $1 == "W" { W = $2 + 0; G = $4 + 0; next }
NR == 2 {
  for (i = 1; i <= W; i++) cell[i] = (substr($0, i, 1) == "#") ? 1 : 0
}

function step(   i, l, c, r, idx, n) {
  for (i = 1; i <= W; i++) {
    l = cell[ (i == 1) ? W : i - 1 ]
    c = cell[i]
    r = cell[ (i == W) ? 1 : i + 1 ]
    idx = l * 4 + c * 2 + r
    # Rule 30 = 0001 1110 (bits 4,3,2,1 are 1; bits 7,6,5,0 are 0).
    if (idx == 0) n = 0
    if (idx == 1) n = 1
    if (idx == 2) n = 1
    if (idx == 3) n = 1
    if (idx == 4) n = 1
    if (idx == 5) n = 0
    if (idx == 6) n = 0
    if (idx == 7) n = 0
    nxt[i] = n
  }
  for (i = 1; i <= W; i++) cell[i] = nxt[i]
}

function show(label,   i, line, live) {
  line = ""; live = 0
  for (i = 1; i <= W; i++) {
    line = line (cell[i] ? "#" : ".")
    if (cell[i]) live++
  }
  print line
  live_seq[label] = live
}

END {
  show(0)
  for (g = 1; g <= G; g++) { step(); show(g) }
  printf "LIVE:"
  for (g = 0; g <= G; g++) printf " %d", live_seq[g]
  print ""
}
