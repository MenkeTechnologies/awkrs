# Gaussian elimination with partial pivoting — solve Ax = b for x.
# Input first line:  "N <n>"
# Next n lines:      n+1 whitespace-separated values per row: the n
#                    coefficients of A followed by the rhs entry of b.
# Output:            "x_1 = v1"  …  "x_n = vn"  with six fractional digits,
#                    or "SINGULAR" when no pivot is found in a column.

NR == 1 && $1 == "N" { N = $2 + 0; row = 0; next }
{
  row++
  for (c = 1; c <= NF; c++) a[row, c] = $c + 0
}

END {
  # forward elimination with partial pivoting
  for (k = 1; k <= N; k++) {
    # find pivot
    best = k
    for (i = k + 1; i <= N; i++) {
      if (((a[i, k] < 0) ? -a[i, k] : a[i, k]) > ((a[best, k] < 0) ? -a[best, k] : a[best, k])) best = i
    }
    if (a[best, k] == 0) { print "SINGULAR"; exit 1 }
    if (best != k) {
      for (c = 1; c <= N + 1; c++) {
        t = a[k, c]; a[k, c] = a[best, c]; a[best, c] = t
      }
    }
    # eliminate below
    for (i = k + 1; i <= N; i++) {
      f = a[i, k] / a[k, k]
      for (c = k; c <= N + 1; c++) a[i, c] -= f * a[k, c]
    }
  }

  # back substitution
  for (i = N; i >= 1; i--) {
    s = a[i, N + 1]
    for (c = i + 1; c <= N; c++) s -= a[i, c] * x[c]
    x[i] = s / a[i, i]
  }

  for (i = 1; i <= N; i++) printf "x_%d = %.6f\n", i, x[i]
}
