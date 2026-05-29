# Tic-tac-toe minimax solver. Given a board position and whose turn it is,
# print the optimal move + outcome with best play from both sides.
# Input lines:
#   "<board9>  <player>"     board9 = 9 chars over {X, O, .}, row-major.
#                            player = X or O — whose move it is.
# Output: "board=<b> turn=<p> best=<idx 1..9> score=<-1|0|1>  ('1' means current
# player wins, '0' draw, '-1' loses)".
# Note: tic-tac-toe is a known draw with perfect play from the start, so the
# empty board returns score=0.

function opponent(p) { return (p == "X") ? "O" : "X" }

function winner(   lines, i, a, b, c, va, vb, vc) {
  lines[1] = "1 2 3"; lines[2] = "4 5 6"; lines[3] = "7 8 9"
  lines[4] = "1 4 7"; lines[5] = "2 5 8"; lines[6] = "3 6 9"
  lines[7] = "1 5 9"; lines[8] = "3 5 7"
  for (i = 1; i <= 8; i++) {
    split(lines[i], idx, " ")
    a = idx[1]; b = idx[2]; c = idx[3]
    va = board[a]; vb = board[b]; vc = board[c]
    if (va != "." && va == vb && vb == vc) return va
  }
  return "."
}

function full(   i) {
  for (i = 1; i <= 9; i++) if (board[i] == ".") return 0
  return 1
}

function minimax(player,   w, i, sc, best, best_idx, saved) {
  w = winner()
  if (w != ".") {
    return (w == root_player) ? 1 : -1
  }
  if (full()) return 0
  best = (player == root_player) ? -100 : 100
  best_idx = 0
  for (i = 1; i <= 9; i++) {
    if (board[i] != ".") continue
    saved = board[i]
    board[i] = player
    sc = minimax(opponent(player))
    board[i] = saved
    if (player == root_player) {
      if (sc > best) { best = sc; best_idx = i }
    } else {
      if (sc < best) { best = sc; best_idx = i }
    }
  }
  if (depth_top) { best_move = best_idx; best_score = best }
  return best
}

{
  delete board
  for (i = 1; i <= 9; i++) board[i] = substr($1, i, 1)
  turn = $2

  root_player = turn
  depth_top = 1
  s = minimax(turn)
  depth_top = 0

  printf "board=%s turn=%s best=%d score=%d\n", $1, turn, best_move, best_score
}
