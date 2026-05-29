# Base64 encode / decode without using `system` or any external tool.
# Input lines:
#   "ENC <text>"      output base64-encoded form of <text>
#   "DEC <b64>"       output decoded text
# Output: "ENC: <text>  -> <b64>"  or  "DEC: <b64>  -> <text>"

BEGIN {
  alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
  for (i = 0; i < 64; i++) {
    ch = substr(alphabet, i + 1, 1)
    enc[i] = ch
    dec[ch] = i
  }
  for (i = 0; i < 256; i++) chr[i] = sprintf("%c", i)
  # Inverse mapping char -> byte value, for the input bytes themselves.
  for (i = 0; i < 256; i++) ord[chr[i]] = i
}

function b64_encode(s,   n, i, b1, b2, b3, out, t) {
  n = length(s); i = 1
  out = ""
  while (i <= n) {
    b1 = ord[substr(s, i, 1)] + 0
    b2 = (i + 1 <= n) ? ord[substr(s, i + 1, 1)] + 0 : -1
    b3 = (i + 2 <= n) ? ord[substr(s, i + 2, 1)] + 0 : -1
    t = enc[int(b1 / 4)]
    if (b2 == -1) {
      t = t enc[(b1 % 4) * 16] "=="
    } else if (b3 == -1) {
      t = t enc[(b1 % 4) * 16 + int(b2 / 16)]
      t = t enc[(b2 % 16) * 4] "="
    } else {
      t = t enc[(b1 % 4) * 16 + int(b2 / 16)]
      t = t enc[(b2 % 16) * 4 + int(b3 / 64)]
      t = t enc[b3 % 64]
    }
    out = out t
    i += 3
  }
  return out
}

function b64_decode(s,   n, i, c1, c2, c3, c4, out, b1, b2, b3, ch) {
  n = length(s); i = 1
  out = ""
  while (i <= n) {
    ch = substr(s, i, 1); c1 = (ch in dec) ? dec[ch] : 0
    ch = substr(s, i + 1, 1); c2 = (ch in dec) ? dec[ch] : 0
    ch = substr(s, i + 2, 1); c3 = (ch == "=") ? -1 : ((ch in dec) ? dec[ch] : 0)
    ch = substr(s, i + 3, 1); c4 = (ch == "=") ? -1 : ((ch in dec) ? dec[ch] : 0)
    b1 = c1 * 4 + int(c2 / 16)
    out = out chr[b1]
    if (c3 != -1) {
      b2 = (c2 % 16) * 16 + int(c3 / 4)
      out = out chr[b2]
      if (c4 != -1) {
        b3 = (c3 % 4) * 64 + c4
        out = out chr[b3]
      }
    }
    i += 4
  }
  return out
}

$1 == "ENC" { txt = substr($0, 5); printf "ENC: %s  -> %s\n", txt, b64_encode(txt); next }
$1 == "DEC" { txt = substr($0, 5); printf "DEC: %s  -> %s\n", txt, b64_decode(txt); next }
