# Levenshtein edit distance via O(la*lb) DP table held in a SUBSEP 2D array.
# Input lines: "<word_a> <word_b>" pairs.
# Output:      "<word_a> <word_b> <distance>"

function lev(a, b,   la, lb, i, j, cost, mn) {
  la = length(a); lb = length(b)
  for (i = 0; i <= la; i++) d[i, 0] = i
  for (j = 0; j <= lb; j++) d[0, j] = j
  for (i = 1; i <= la; i++) {
    for (j = 1; j <= lb; j++) {
      cost = (substr(a, i, 1) == substr(b, j, 1)) ? 0 : 1
      mn = d[i - 1, j] + 1
      if (d[i, j - 1] + 1     < mn) mn = d[i, j - 1] + 1
      if (d[i - 1, j - 1] + cost < mn) mn = d[i - 1, j - 1] + cost
      d[i, j] = mn
    }
  }
  return d[la, lb]
}

{
  printf "%s %s %d\n", $1, $2, lev($1, $2)
  delete d
}
