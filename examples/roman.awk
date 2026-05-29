# Roman numerals ↔ integers.
# Each input line is one token. If it's a positive integer it's converted to
# Roman; if it's a Roman string (case-insensitive) it's converted to integer.
# Subtractive form supported (IV, IX, XL, XC, CD, CM). Range 1..3999.

BEGIN {
  # Decimal -> Roman tables in descending order.
  n = 0
  n++; vals[n] = 1000; syms[n] = "M"
  n++; vals[n] = 900;  syms[n] = "CM"
  n++; vals[n] = 500;  syms[n] = "D"
  n++; vals[n] = 400;  syms[n] = "CD"
  n++; vals[n] = 100;  syms[n] = "C"
  n++; vals[n] = 90;   syms[n] = "XC"
  n++; vals[n] = 50;   syms[n] = "L"
  n++; vals[n] = 40;   syms[n] = "XL"
  n++; vals[n] = 10;   syms[n] = "X"
  n++; vals[n] = 9;    syms[n] = "IX"
  n++; vals[n] = 5;    syms[n] = "V"
  n++; vals[n] = 4;    syms[n] = "IV"
  n++; vals[n] = 1;    syms[n] = "I"
  nsyms = n

  # Single-letter Roman digit values for parsing.
  rv["I"] = 1;   rv["V"] = 5;    rv["X"] = 10;   rv["L"] = 50
  rv["C"] = 100; rv["D"] = 500;  rv["M"] = 1000
}

function to_roman(x,   i, out) {
  if (x < 1 || x > 3999) return "ERR(out of range 1..3999)"
  out = ""
  for (i = 1; i <= nsyms; i++) {
    while (x >= vals[i]) { out = out syms[i]; x -= vals[i] }
  }
  return out
}

function to_int(s,   i, total, ch, nx, vi, vnext) {
  s = toupper(s)
  total = 0
  for (i = 1; i <= length(s); i++) {
    ch = substr(s, i, 1)
    if (!(ch in rv)) return "ERR(bad char " ch ")"
    vi = rv[ch]
    nx = substr(s, i + 1, 1)
    vnext = (nx in rv) ? rv[nx] : 0
    if (vnext > vi) total -= vi
    else total += vi
  }
  return total
}

NF == 0 { next }

{
  t = $1
  if (t ~ /^[0-9]+$/) printf "%s -> %s\n", t, to_roman(t + 0)
  else                printf "%s -> %s\n", t, to_int(t)
}
