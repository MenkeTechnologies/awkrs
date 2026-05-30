# Quickselect — kth smallest element via Hoare partitioning.
# Input first line:  "K <k>"   the rank to find (1-based)
# Second line:       whitespace-separated integers
# Output:            "input: a1 a2 ...   k=<k>   kth_smallest=<v>"
# Tie-break: Lomuto partition with middle-of-range pivot — deterministic, no rand.

function partition(lo, hi,   piv, i, j, tmp) {
  piv = arr[lo + int((hi - lo) / 2)]
  i = lo - 1; j = hi + 1
  while (1) {
    do { i++ } while (arr[i] < piv)
    do { j-- } while (arr[j] > piv)
    if (i >= j) return j
    tmp = arr[i]; arr[i] = arr[j]; arr[j] = tmp
  }
}

function select(lo, hi, k,   p) {
  while (lo < hi) {
    p = partition(lo, hi)
    if (k <= p) hi = p
    else        lo = p + 1
  }
  return arr[lo]
}

NR == 1 && $1 == "K" { K = $2 + 0; next }
NR == 2 {
  n = NF
  for (i = 1; i <= n; i++) arr[i] = $i + 0
  raw = $0
  v = select(1, n, K)
  printf "input: %s   k=%d   kth_smallest=%d\n", raw, K, v
  exit 0
}
