# Iterative segment tree (point update, range sum query) on an array of N.
# Input first line:  "N <n>"
# Second line:       n whitespace-separated initial values
# Subsequent lines:
#   "SUM <l> <r>"    print "SUM l r -> <sum>" (1-based, inclusive)
#   "SET <i> <v>"    set position i to v
#   "ADD <i> <v>"    add v to position i (acknowledged silently)
# Tree stored 1-indexed in t[1..2N-1] with leaves at indices N..2N-1.

NR == 1 && $1 == "N" { N = $2 + 0; next }
NR == 2 {
  for (i = 1; i <= N; i++) t[N + i - 1] = $i + 0
  for (i = N - 1; i >= 1; i--) t[i] = t[2 * i] + t[2 * i + 1]
  next
}

function point_set(i, v,   p) {
  p = N + i - 1
  t[p] = v
  p = int(p / 2)
  while (p >= 1) { t[p] = t[2 * p] + t[2 * p + 1]; p = int(p / 2) }
}

function point_add(i, v,   p) {
  p = N + i - 1
  t[p] += v
  p = int(p / 2)
  while (p >= 1) { t[p] = t[2 * p] + t[2 * p + 1]; p = int(p / 2) }
}

function range_sum(l, r,   sum, a, b) {
  sum = 0
  a = N + l - 1
  b = N + r - 1
  while (a <= b) {
    if (a % 2 == 1)  { sum += t[a]; a++ }
    if (b % 2 == 0)  { sum += t[b]; b-- }
    a = int(a / 2)
    b = int(b / 2)
  }
  return sum
}

$1 == "SUM" { printf "SUM %d %d -> %d\n", $2, $3, range_sum($2 + 0, $3 + 0); next }
$1 == "SET" { point_set($2 + 0, $3 + 0); next }
$1 == "ADD" { point_add($2 + 0, $3 + 0); next }
