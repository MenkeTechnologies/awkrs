# Suffix array — list every suffix of the input string sorted lex.
# O(n^2 log n) trivial sort (build all suffixes, sort) — plenty fast for the
# demo inputs and easy to read. Output:
#   "<input>: SA=[i1 i2 ...]"   1-based starting positions of suffixes in sort order
#   "      LCP=[l1 l2 ...]"     longest-common-prefix between consecutive suffixes
#
# For each input line that's non-empty.

function suffix_at(s, i) { return substr(s, i) }

function lcp(a, b,   k, n) {
  n = (length(a) < length(b)) ? length(a) : length(b)
  k = 0
  while (k < n && substr(a, k + 1, 1) == substr(b, k + 1, 1)) k++
  return k
}

NF == 0 { next }

{
  s = $0
  n = length(s)
  delete suf
  for (i = 1; i <= n; i++) suf[i] = sprintf("%s\t%d", suffix_at(s, i), i)
  asort(suf)

  sa = ""; lp = ""
  for (i = 1; i <= n; i++) {
    split(suf[i], parts, "\t")
    sa = sa (i == 1 ? "" : " ") parts[2]
    if (i >= 2) {
      split(suf[i - 1], prev, "\t")
      split(suf[i],     cur,  "\t")
      lp = lp (i == 2 ? "" : " ") lcp(prev[1], cur[1])
    }
  }
  printf "%s: SA=[%s]\n", s, sa
  printf "      LCP=[%s]\n", lp
}
