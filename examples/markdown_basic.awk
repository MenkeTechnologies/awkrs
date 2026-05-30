# Tiny Markdown → HTML for a hand-picked subset:
#   # heading      → <h1>..</h1>  (up to 6 levels)
#   **bold**       → <strong>..</strong>
#   *italic*       → <em>..</em>
#   `code`         → <code>..</code>
#   [text](url)    → <a href="url">text</a>
#   - item         → <ul><li>..</li></ul>     (consecutive items merge into a list)
#   ```            → <pre>..</pre>            (fenced code block)
#   blank line     → paragraph break
# Anything else becomes <p>…</p>. Not a full Markdown engine — strictly a
# demonstration of awk's line-oriented transformation style.

function inline(s) {
  s = gensub(/\*\*([^*]+)\*\*/, "<strong>\\1</strong>", "g", s)
  s = gensub(/\*([^*]+)\*/, "<em>\\1</em>", "g", s)
  s = gensub(/`([^`]+)`/, "<code>\\1</code>", "g", s)
  s = gensub(/\[([^\]]+)\]\(([^)]+)\)/, "<a href=\"\\2\">\\1</a>", "g", s)
  return s
}

BEGIN { in_ul = 0; in_code = 0 }

function flush_ul() { if (in_ul) { print "</ul>"; in_ul = 0 } }

/^```/ {
  if (in_code) { print "</pre>"; in_code = 0 } else { flush_ul(); print "<pre>"; in_code = 1 }
  next
}

in_code { print $0; next }

NF == 0 { flush_ul(); next }

/^#{1,6} / {
  flush_ul()
  level = 0
  while (substr($0, level + 1, 1) == "#") level++
  rest = substr($0, level + 2)
  printf "<h%d>%s</h%d>\n", level, inline(rest), level
  next
}

/^- / {
  if (!in_ul) { print "<ul>"; in_ul = 1 }
  printf "<li>%s</li>\n", inline(substr($0, 3))
  next
}

{
  flush_ul()
  printf "<p>%s</p>\n", inline($0)
}

END { flush_ul(); if (in_code) print "</pre>" }
