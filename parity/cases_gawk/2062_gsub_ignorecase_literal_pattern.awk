# gawk parity: when `IGNORECASE` is set, sub/gsub honor it even for plain
# string patterns. awkrs's literal-pattern fast path used to bypass regex
# compilation, so `gsub("b", "X", "ABC")` silently failed to match.
BEGIN {
    IGNORECASE = 1
    s = "ABCabc"
    gsub("b", "X", s)
    print s

    t = "FOOfoo"
    sub("FoO", "Y", t)
    print t

    # Match with literal needle (no regex metachars).
    u = "Hello World"
    n = gsub("hello", "Hi", u)
    print n, u
}
