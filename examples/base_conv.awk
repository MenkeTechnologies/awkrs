# Base conversion 2..36 in either direction, no `strtonum` / no `%x` reliance.
# Input lines: "<from_base> <to_base> <value>".
#   from_base / to_base in 2..36; <value> uses 0-9 then A-Z for digit > 9
#   (case-insensitive).
# Output: "<value> [base <from>] -> <result> [base <to>]"
# A leading '-' on <value> is preserved.

BEGIN {
  digs = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
  for (i = 1; i <= length(digs); i++) {
    ch = substr(digs, i, 1)
    val[ch] = i - 1
    val[tolower(ch)] = i - 1
  }
}

function to_int(s, base,   neg, n, i, c, total) {
  neg = 0
  if (substr(s, 1, 1) == "-") { neg = 1; s = substr(s, 2) }
  total = 0
  n = length(s)
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1)
    if (!(c in val) || val[c] >= base) return "ERR(bad digit " c ")"
    total = total * base + val[c]
  }
  return neg ? -total : total
}

function from_int(x, base,   out, neg, r) {
  if (x == 0) return "0"
  neg = (x < 0)
  if (neg) x = -x
  out = ""
  while (x > 0) {
    r = x % base
    out = substr(digs, r + 1, 1) out
    x = int(x / base)
  }
  if (neg) out = "-" out
  return out
}

NF == 3 {
  from = $1 + 0; to = $2 + 0; src = $3
  if (from < 2 || from > 36 || to < 2 || to > 36) {
    printf "%s [base %d] -> ERR(base out of range)\n", src, from
    next
  }
  n = to_int(src, from)
  if (n ~ /^ERR/) printf "%s [base %d] -> %s\n", src, from, n
  else            printf "%s [base %d] -> %s [base %d]\n", src, from, from_int(n, to), to
}
