# 2048 board: apply a sequence of moves (L, R, U, D) to a 4x4 grid and
# print the board + score after each move. No new tile is spawned —
# this isolates the merge logic so the example stays parity-safe (no rand).
# Input lines:
#   "BOARD" (line 1) "BOARD <16 numbers>"
#   "MOVES <chars>"  where each char is L, R, U, D
# A 0 cell is empty. Tiles merge in pairs at most once per move per row/col.
# After each move:
#   "after <move>: score=<s>"
#   four lines of four right-aligned cells

NR == 1 && $1 == "BOARD" {
  for (i = 1; i <= 16; i++) b[i] = $(i + 1) + 0
  score = 0
  print_board("init")
  next
}

function pos(r, c) { return (r - 1) * 4 + c }

function print_board(label,   r, c, line) {
  printf "after %s: score=%d\n", label, score
  for (r = 1; r <= 4; r++) {
    line = ""
    for (c = 1; c <= 4; c++) line = line sprintf("%5d", b[pos(r, c)])
    print line
  }
}

function compress_left(   r, c, lane, k, i, out, last_merge) {
  for (r = 1; r <= 4; r++) {
    delete lane
    k = 0
    for (c = 1; c <= 4; c++) {
      if (b[pos(r, c)] != 0) lane[++k] = b[pos(r, c)]
    }
    delete out
    i = 1; out_n = 0; last_merge = 0
    while (i <= k) {
      if (i < k && lane[i] == lane[i + 1] && last_merge != i) {
        out_n++; out[out_n] = lane[i] * 2; score += out[out_n]
        last_merge = out_n
        i += 2
      } else {
        out_n++; out[out_n] = lane[i]
        i++
      }
    }
    for (c = 1; c <= 4; c++) b[pos(r, c)] = (c <= out_n) ? out[c] : 0
  }
}

function reflect_h(   r, c, t) {
  for (r = 1; r <= 4; r++) {
    t = b[pos(r,1)]; b[pos(r,1)] = b[pos(r,4)]; b[pos(r,4)] = t
    t = b[pos(r,2)]; b[pos(r,2)] = b[pos(r,3)]; b[pos(r,3)] = t
  }
}

function transpose(   r, c, t) {
  for (r = 1; r <= 4; r++) for (c = r + 1; c <= 4; c++) {
    t = b[pos(r, c)]; b[pos(r, c)] = b[pos(c, r)]; b[pos(c, r)] = t
  }
}

function move(m) {
  if (m == "L") { compress_left() }
  if (m == "R") { reflect_h(); compress_left(); reflect_h() }
  if (m == "U") { transpose(); compress_left(); transpose() }
  if (m == "D") { transpose(); reflect_h(); compress_left(); reflect_h(); transpose() }
}

$1 == "MOVES" {
  seq = $2
  for (i = 1; i <= length(seq); i++) {
    m = substr(seq, i, 1)
    move(m)
    print_board(m)
  }
  next
}
