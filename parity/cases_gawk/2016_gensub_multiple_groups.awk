BEGIN {
    s = "alice bob carol"
    r = gensub(/(\w+) (\w+) (\w+)/, "\\3,\\2,\\1", "g", s)
    print r
}
