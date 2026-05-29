# Heap's algorithm — generate all n! permutations of the input tokens.
# Input first line:  "N <n>"
# Second line:       n whitespace-separated tokens
# Output: every permutation on its own line, space-separated; then
#         "COUNT: <n!>".
# Recursion bound matches Heap's swap pattern (~k * (k-1)! recursive calls).

function swap(i, j,   t) { t = arr[i]; arr[i] = arr[j]; arr[j] = t }

function permute(k,   i, line) {
  if (k == 1) {
    line = arr[1]
    for (i = 2; i <= N; i++) line = line " " arr[i]
    print line
    count++
    return
  }
  for (i = 1; i <= k; i++) {
    permute(k - 1)
    if (k % 2 == 0) swap(i, k)
    else            swap(1, k)
  }
}

NR == 1 && $1 == "N" { N = $2 + 0; next }
NR == 2 {
  for (i = 1; i <= N; i++) arr[i] = $i
  permute(N)
  printf "COUNT: %d\n", count
  exit 0
}
