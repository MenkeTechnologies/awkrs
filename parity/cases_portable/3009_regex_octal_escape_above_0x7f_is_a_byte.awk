
# portable:3009 — an octal escape above \177 in a regex is a byte, inside and
# outside a bracket expression, so it matches the byte a string escape makes.
BEGIN {
    s = "a\303\251b"
    print ("\303" ~ /\303/), ("\303" ~ /^\303$/), ("\303" ~ /[\303]/)
    print ("\351" ~ /\351/), ("x\377y" ~ /x[\376\377]y/), ("A" ~ /\101/)
    print match(s, /\303\251/), match("x\351", /\351/)
    n = gsub(/\303\251/, "e", s); print n, s
}
