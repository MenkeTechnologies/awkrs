# gawk parity for sub/gsub replacement-string semantics:
#   `&`     → match
#   `\&`    → literal `&`
#   `\\&`   → literal `\` + match
#   `\\`    → `\\` kept verbatim outside the `&` context
#   `\X`    → `\X` kept verbatim (sub/gsub never expand backrefs — that's gensub)
BEGIN {
    s = "AbB"
    t = s; gsub(/B/, "X",     t); print 1, "[" t "]"
    t = s; gsub(/B/, "&",     t); print 2, "[" t "]"
    t = s; gsub(/B/, "\\&",   t); print 3, "[" t "]"
    t = s; gsub(/B/, "\\\\",  t); print 4, "[" t "]"
    t = s; gsub(/B/, "\\\\&", t); print 5, "[" t "]"
    t = s; gsub(/B/, "\\X",   t); print 6, "[" t "]"

    # Capture group in pattern + \1 in replacement: stays literal (only gensub
    # supports backrefs).
    u = "abc"
    sub(/(a)/, "[\\1]", u)
    print 7, "[" u "]"
}
