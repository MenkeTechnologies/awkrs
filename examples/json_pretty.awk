# JSON pretty-printer (2-space indent).
# Tokenizes the whole input into JSON tokens, then re-emits with structural
# indentation. Strings, numbers, true/false/null, {}, [], commas, colons.
# Strings keep their original bytes verbatim (escapes preserved).
# Assumes well-formed input; not a validator.

function tokenize(s,   n, i, c, t, j, depth) {
  n = length(s)
  i = 1
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == " " || c == "\t" || c == "\n" || c == "\r") { i++; continue }
    if (c == "{" || c == "}" || c == "[" || c == "]" || c == ":" || c == ",") {
      tk[++ntk] = c; i++; continue
    }
    if (c == "\"") {
      t = "\""; j = i + 1
      while (j <= n) {
        cc = substr(s, j, 1)
        if (cc == "\\") { t = t cc substr(s, j + 1, 1); j += 2; continue }
        if (cc == "\"") { t = t "\""; j++; break }
        t = t cc; j++
      }
      tk[++ntk] = t; i = j; continue
    }
    # number or literal (true/false/null) — greedy scan until structural
    t = ""
    while (i <= n) {
      cc = substr(s, i, 1)
      if (cc == "," || cc == "}" || cc == "]" || cc == " " || cc == "\t" || cc == "\n" || cc == "\r" || cc == ":") break
      t = t cc; i++
    }
    if (t != "") tk[++ntk] = t
  }
}

function pad(d,   s, k) { s = ""; for (k = 0; k < d; k++) s = s "  "; return s }

BEGIN { buf = "" }

{ buf = buf $0 "\n" }

END {
  ntk = 0
  tokenize(buf)

  depth = 0
  out = ""
  for (i = 1; i <= ntk; i++) {
    t = tk[i]; nx = (i < ntk) ? tk[i + 1] : ""
    if (t == "{" || t == "[") {
      out = out t
      if (nx == "}" || nx == "]") {
        # empty container — keep on one line
      } else {
        depth++
        out = out "\n" pad(depth)
      }
      continue
    }
    if (t == "}" || t == "]") {
      # Was the previous token the matching open? Then container was empty.
      pv = tk[i - 1]
      if (!((t == "}" && pv == "{") || (t == "]" && pv == "["))) {
        depth--
        out = out "\n" pad(depth)
      }
      out = out t
      continue
    }
    if (t == ",") {
      out = out ",\n" pad(depth)
      continue
    }
    if (t == ":") {
      out = out ": "
      continue
    }
    out = out t
  }
  print out
}
