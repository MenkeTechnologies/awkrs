# Fenwick / Binary Indexed Tree for prefix sums.
# Supports point update + prefix-sum / range-sum queries in O(log N).
#
# Input first line: "N <n>"
# Subsequent lines:
#   "ADD <i> <v>"       BIT[i] += v (1-based)
#   "PSUM <i>"          print sum of [1..i]   -> "PSUM i -> <s>"
#   "RSUM <l> <r>"      print sum of [l..r]   -> "RSUM l r -> <s>"
#
# bit[1..N] is the tree.

function lowbit(x) { return x - and(x, x - 1) }   # gawk rejects negative bit-op args

function bit_update(i, v) {
  while (i <= N) { bit[i] += v; i += lowbit(i) }
}

function bit_prefix(i,   s) {
  s = 0
  while (i > 0) { s += bit[i]; i -= lowbit(i) }
  return s
}

NR == 1 && $1 == "N" {
  N = $2 + 0
  for (i = 1; i <= N; i++) bit[i] = 0
  next
}

$1 == "ADD"  { bit_update($2 + 0, $3 + 0); next }
$1 == "PSUM" { printf "PSUM %d -> %d\n", $2 + 0, bit_prefix($2 + 0); next }
$1 == "RSUM" { printf "RSUM %d %d -> %d\n", $2 + 0, $3 + 0, bit_prefix($3 + 0) - bit_prefix($2 - 1); next }
