# Arbitrary-precision integer multiplication via schoolbook (O(m*n) digit ops).
# Input lines: "<a> <b>"   (positive or negative integers, any length)
# Output:      "<a> * <b> = <product>"
#
# Numbers are stored as digit arrays indexed 1..length, least-significant first
# (handy for accumulation). Sign tracked separately.

function strip_lead(s,   i) {
  i = 1
  while (i < length(s) && substr(s, i, 1) == "0") i++
  return substr(s, i)
}

function mul_pos(a, b,   la, lb, i, j, carry, k, prod, out) {
  la = length(a); lb = length(b)
  # ra[i] = LSB-first digit of a
  for (i = 1; i <= la; i++) ra[i] = substr(a, la - i + 1, 1) + 0
  for (i = 1; i <= lb; i++) rb[i] = substr(b, lb - i + 1, 1) + 0
  for (i = 1; i <= la + lb; i++) acc[i] = 0
  for (i = 1; i <= la; i++) {
    for (j = 1; j <= lb; j++) {
      acc[i + j - 1] += ra[i] * rb[j]
    }
  }
  for (k = 1; k <= la + lb; k++) {
    if (acc[k] >= 10) {
      carry = int(acc[k] / 10)
      acc[k] %= 10
      acc[k + 1] += carry
    }
  }
  out = ""
  for (k = la + lb; k >= 1; k--) out = out acc[k]
  out = strip_lead(out)
  delete ra; delete rb; delete acc
  return out
}

{
  a = $1; b = $2
  sa = 1; sb = 1
  if (substr(a, 1, 1) == "-") { sa = -1; a = substr(a, 2) }
  if (substr(b, 1, 1) == "-") { sb = -1; b = substr(b, 2) }
  p = mul_pos(a, b)
  if (p == "0") sign = ""
  else if (sa * sb == -1) sign = "-"
  else sign = ""
  printf "%s * %s = %s%s\n", $1, $2, sign, p
}
