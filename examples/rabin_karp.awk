# Rabin-Karp substring search with rolling polynomial hash.
# Input lines:
#   "PAT <p>"        set / reset the pattern
#   "TXT <t>"        find all 1-based positions in <t>; print "<t> -> p1 p2 ..."
#                    or "NONE". On any hash collision the match is double-
#                    checked with substr() so output is byte-exact even though
#                    the example is hash-driven.
#
# Hash: H(s) = sum_i s[i] * BASE^(m-i)  mod M.
# Constants chosen so awk's f64 holds intermediate products without precision
# loss for the demo inputs (BASE * M fits in 2^52).

BEGIN {
  BASE = 257
  MOD = 1000003
  for (i = 0; i < 256; i++) ord[sprintf("%c", i)] = i
}

function pow_mod(base, e, m,   r) {
  r = 1
  while (e > 0) {
    if (e % 2 == 1) r = (r * base) % m
    base = (base * base) % m
    e = int(e / 2)
  }
  return r
}

function rk_search(pat, txt,   m, n, i, ph, th, top, hits, sep, ok) {
  m = length(pat); n = length(txt)
  if (m == 0 || m > n) return ""
  ph = 0; th = 0
  for (i = 1; i <= m; i++) {
    ph = (ph * BASE + ord[substr(pat, i, 1)]) % MOD
    th = (th * BASE + ord[substr(txt, i, 1)]) % MOD
  }
  top = pow_mod(BASE, m - 1, MOD)
  hits = ""; sep = ""
  for (i = 1; i + m - 1 <= n; i++) {
    if (ph == th) {
      if (substr(txt, i, m) == pat) {
        hits = hits sep i
        sep = " "
      }
    }
    if (i + m <= n) {
      th = (th - ord[substr(txt, i, 1)] * top) % MOD
      if (th < 0) th += MOD
      th = (th * BASE + ord[substr(txt, i + m, 1)]) % MOD
    }
  }
  return hits
}

$1 == "PAT" { pat = substr($0, 5); next }
$1 == "TXT" {
  txt = substr($0, 5)
  h = rk_search(pat, txt)
  printf "%s -> %s\n", txt, (h == "" ? "NONE" : h)
  next
}
