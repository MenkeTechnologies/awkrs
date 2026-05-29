# Vigenère cipher (encrypt / decrypt). A-Z and a-z are shifted by the key;
# non-letters pass through verbatim. Key cycles letter-by-letter, case-folded.
#
# Input lines:
#   "ENC <key> <text>"   encrypt
#   "DEC <key> <text>"   decrypt

BEGIN {
  for (i = 0; i < 256; i++) {
    ch = sprintf("%c", i)
    ord[ch] = i
    chr[i]  = ch
  }
}

function shift_char(c, k, dir,   o, base, n) {
  o = ord[c]
  if (o >= 65 && o <= 90)      base = 65
  else if (o >= 97 && o <= 122) base = 97
  else return c
  n = ((o - base) + dir * k + 2600) % 26   # +2600 keeps modulus positive
  return chr[base + n]
}

function vig(key, text, dir,   ki, klen, i, n, out, c, kc, ko) {
  klen = length(key)
  ki = 0
  out = ""
  n = length(text)
  for (i = 1; i <= n; i++) {
    c = substr(text, i, 1)
    if (c ~ /[A-Za-z]/) {
      kc = substr(key, (ki % klen) + 1, 1)
      ko = ord[kc]
      if (ko >= 97 && ko <= 122) ko -= 32   # fold key to upper
      out = out shift_char(c, ko - 65, dir)
      ki++
    } else {
      out = out c
    }
  }
  return out
}

{
  if ($1 == "ENC") { key = $2; txt = substr($0, length($1) + length($2) + 3); printf "ENC %s: %s  -> %s\n", key, txt, vig(key, txt,  1); next }
  if ($1 == "DEC") { key = $2; txt = substr($0, length($1) + length($2) + 3); printf "DEC %s: %s  -> %s\n", key, txt, vig(key, txt, -1); next }
}
