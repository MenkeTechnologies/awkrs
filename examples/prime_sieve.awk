# Sieve of Eratosthenes up to N.
# Input: one line "N".
# Output: all primes p <= N, ten per row; then "COUNT: <pi(N)>".
# Uses a sparse delete-marked array; iteration order doesn't matter — we
# walk 2..N in index order.

NR == 1 {
  N = $1 + 0
  if (N < 2) { print "COUNT: 0"; exit 0 }
  for (i = 2; i <= N; i++) is[i] = 1
  for (i = 2; i * i <= N; i++) {
    if (!(i in is)) continue
    if (is[i] == 0) continue
    for (j = i * i; j <= N; j += i) is[j] = 0
  }
  col = 0
  for (i = 2; i <= N; i++) {
    if (is[i] == 1) {
      printf "%6d", i
      col++
      if (col % 10 == 0) print ""
      cnt++
    }
  }
  if (col % 10 != 0) print ""
  printf "COUNT: %d\n", cnt
  exit 0
}
