# Run-length codec.
# Input lines:
#   "ENC <text>"     → "ENC: <text>  -> <encoded>"
#   "DEC <coded>"    → "DEC: <coded>  -> <decoded>"
#
# Encoded form: <count><char> for every run; counts are decimal, no count = 1.
# Whitespace inside <text> is preserved verbatim (spaces count as runs too).
# Decoding accepts both "<count><char>" and bare "<char>" (count 1).

function rle_encode(s,   n, i, cur, run, out) {
  n = length(s)
  if (n == 0) return ""
  cur = substr(s, 1, 1); run = 1
  out = ""
  for (i = 2; i <= n; i++) {
    c = substr(s, i, 1)
    if (c == cur) { run++; continue }
    out = out (run > 1 ? run : "") cur
    cur = c; run = 1
  }
  out = out (run > 1 ? run : "") cur
  return out
}

function rle_decode(s,   n, i, c, num, out, j) {
  n = length(s)
  out = ""
  i = 1
  while (i <= n) {
    c = substr(s, i, 1)
    if (c ~ /[0-9]/) {
      num = ""
      while (i <= n && substr(s, i, 1) ~ /[0-9]/) { num = num substr(s, i, 1); i++ }
      if (i > n) return out
      c = substr(s, i, 1); i++
      for (j = 0; j < (num + 0); j++) out = out c
    } else {
      out = out c
      i++
    }
  }
  return out
}

$1 == "ENC" { txt = substr($0, 5); printf "ENC: %s  -> %s\n", txt, rle_encode(txt); next }
$1 == "DEC" { txt = substr($0, 5); printf "DEC: %s  -> %s\n", txt, rle_decode(txt); next }
