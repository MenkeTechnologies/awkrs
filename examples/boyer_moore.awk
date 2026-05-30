# Boyer-Moore string search using the bad-character heuristic.
# The good-suffix rule is omitted to keep the script short — bad-char alone is
# the classic single-table BM and is enough to demonstrate the right-to-left
# scan with character-driven shifts.
#
# Input lines:
#   "PAT <p>"        set / reset the pattern
#   "TXT <t>"        find every match in <t>; print "<t> -> p1 p2 ..."
#                    (1-based start positions) or "<t> -> NONE"
#
# Shift table: last[c] = rightmost 1-based position of c in the pattern
#              (absent = 0).

function bm_preprocess(p,   m, i) {
  m = length(p)
  patlen = m
  delete last
  for (i = 1; i <= m; i++) last[substr(p, i, 1)] = i
}

function bm_search(t,   n, m, s, j, ch, shift, hits, sep) {
  m = patlen; n = length(t)
  if (m == 0 || m > n) return ""
  hits = ""; sep = ""
  s = 0   # 0-based shift of pattern over text
  while (s <= n - m) {
    j = m
    while (j >= 1 && substr(p, j, 1) == substr(t, s + j, 1)) j--
    if (j == 0) {
      hits = hits sep (s + 1); sep = " "
      s++
    } else {
      ch = substr(t, s + j, 1)
      shift = j - ((ch in last) ? last[ch] : 0)
      if (shift < 1) shift = 1
      s += shift
    }
  }
  return hits
}

$1 == "PAT" { p = substr($0, 5); bm_preprocess(p); next }
$1 == "TXT" {
  t = substr($0, 5)
  h = bm_search(t)
  printf "%s -> %s\n", t, (h == "" ? "NONE" : h)
  next
}
