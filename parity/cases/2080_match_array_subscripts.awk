# gawk parity: `match(str, regex, arr)` writes per-group character-indexed
# `arr[i, "start"]` (1-based) and `arr[i, "length"]` for each successful
# submatch. Unmatched optional groups (e.g. `(a)?`) get NO entries.
BEGIN {
    match("foo bar baz", /(\w+) (\w+)/, m)
    print m[0], m[0,"start"], m[0,"length"]
    print m[1], m[1,"start"], m[1,"length"]
    print m[2], m[2,"start"], m[2,"length"]

    # Unmatched optional group: m[1] is absent, only m[2] is populated.
    match("hello", /(a)?(l)+/, n)
    print n[0], n[0,"start"], n[0,"length"]
    print "[" n[1] "]", "[" n[1,"start"] "]", "[" n[1,"length"] "]"
    print n[2], n[2,"start"], n[2,"length"]
}
