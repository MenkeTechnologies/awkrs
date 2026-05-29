# `xxd`-style hex+ASCII dump of input bytes (ASCII range only — non-printables
# render as '.'). Records are 16 bytes wide, address on the left, hex middle,
# printable column on the right.
#
# Reads $0 per line and reinserts the stripped newline so byte offsets match
# the original input. Final partial row is padded to 16.

function emit_row(off, bytes,   i, n, c, hex, asc, glyph, gap) {
  n = length(bytes)
  hex = ""; asc = ""
  for (i = 1; i <= 16; i++) {
    if (i <= n) {
      c = substr(bytes, i, 1)
      hex = hex sprintf("%02x", _ord[c])
      glyph = (_ord[c] >= 32 && _ord[c] < 127) ? c : "."
      asc = asc glyph
    } else {
      hex = hex "  "
      asc = asc " "
    }
    gap = (i == 8) ? "  " : " "
    hex = hex gap
  }
  printf "%08x  %s |%s|\n", off, hex, asc
}

BEGIN {
  for (i = 0; i < 256; i++) _ord[sprintf("%c", i)] = i
  buf = ""
}

{ buf = buf $0 "\n" }

END {
  # Drop the trailing \n appended after the last record so it matches the
  # original input byte count (awk strips the input terminator already).
  n = length(buf)
  if (n > 0 && substr(buf, n, 1) == "\n") buf = substr(buf, 1, n - 1)
  n = length(buf)
  off = 0
  while (off < n) {
    chunk = substr(buf, off + 1, 16)
    emit_row(off, chunk)
    off += 16
  }
  if (n == 0) emit_row(0, "")
  printf "%08x\n", n
}
