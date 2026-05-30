# LZW (Lempel-Ziv-Welch) compression encode + round-trip decode for ASCII
# strings. Dictionary starts with all 256 single-byte codes; new codes are
# appended for w+c each time a known w hits a new c.
#
# Input lines:
#   "ENC <text>"   prints "ENC: <text>  -> <codes>"
#   "DEC <codes>"  decodes a space-separated code list back to text
#                  prints "DEC: <codes>  -> <text>"

BEGIN {
  for (i = 0; i < 256; i++) chr[i] = sprintf("%c", i)
  for (i = 0; i < 256; i++) ord[chr[i]] = i
}

function lzw_enc(s,   n, dict, next_code, w, i, c, wc, codes, sep) {
  delete dict
  for (i = 0; i < 256; i++) dict[chr[i]] = i
  next_code = 256
  n = length(s)
  if (n == 0) return ""
  w = substr(s, 1, 1)
  codes = ""; sep = ""
  for (i = 2; i <= n; i++) {
    c = substr(s, i, 1)
    wc = w c
    if (wc in dict) { w = wc; continue }
    codes = codes sep dict[w]; sep = " "
    dict[wc] = next_code++
    w = c
  }
  codes = codes sep dict[w]
  return codes
}

function lzw_dec(codes,   nk, k, i, prev, entry, c, next_code, out) {
  nk = split(codes, tok, " ")
  if (nk == 0) return ""
  delete dict_idx
  for (i = 0; i < 256; i++) dict_idx[i] = chr[i]
  next_code = 256
  k = tok[1] + 0
  prev = dict_idx[k]
  out = prev
  for (i = 2; i <= nk; i++) {
    k = tok[i] + 0
    if (k in dict_idx) entry = dict_idx[k]
    else if (k == next_code) entry = prev substr(prev, 1, 1)
    else return "ERR(bad code " k ")"
    out = out entry
    dict_idx[next_code++] = prev substr(entry, 1, 1)
    prev = entry
  }
  return out
}

$1 == "ENC" { txt = substr($0, 5); printf "ENC: %s  -> %s\n", txt, lzw_enc(txt); next }
$1 == "DEC" { txt = substr($0, 5); printf "DEC: %s  -> %s\n", txt, lzw_dec(txt); next }
