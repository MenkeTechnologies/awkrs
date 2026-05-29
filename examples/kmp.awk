# Knuth-Morris-Pratt substring search.
# Input lines:
#   "PAT <pattern>"      set / reset the pattern (failure function rebuilt)
#   "TXT <text>"         find every occurrence in <text>; print "<text>: <p1> <p2> ..."
#                        (1-based positions; empty list → "NONE")
#
# Failure function fail[i] stored in a global associative array, rebuilt
# whenever the pattern changes.

function build_fail(p,   m, i, k) {
  m = length(p)
  delete fail
  fail[1] = 0
  k = 0
  for (i = 2; i <= m; i++) {
    while (k > 0 && substr(p, k + 1, 1) != substr(p, i, 1)) k = fail[k]
    if (substr(p, k + 1, 1) == substr(p, i, 1)) k++
    fail[i] = k
  }
}

function kmp_search(t, p,   n, m, q, i, hits) {
  n = length(t); m = length(p)
  if (m == 0 || n == 0) return ""
  q = 0
  hits = ""
  for (i = 1; i <= n; i++) {
    while (q > 0 && substr(p, q + 1, 1) != substr(t, i, 1)) q = fail[q]
    if (substr(p, q + 1, 1) == substr(t, i, 1)) q++
    if (q == m) {
      hits = hits ((hits == "") ? "" : " ") (i - m + 1)
      q = fail[q]
    }
  }
  return hits
}

$1 == "PAT" {
  pat = substr($0, 5)
  build_fail(pat)
  printf "PAT set: %s  (failure: ", pat
  sep = ""
  for (i = 1; i <= length(pat); i++) { printf "%s%d", sep, fail[i]; sep = "," }
  print ")"
  next
}

$1 == "TXT" {
  txt = substr($0, 5)
  h = kmp_search(txt, pat)
  printf "%s -> %s\n", txt, (h == "") ? "NONE" : h
  next
}
