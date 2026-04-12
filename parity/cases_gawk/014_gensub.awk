BEGIN {
    # gensub with global replacement
    print gensub(/[0-9]+/, "NUM", "g", "abc123def456")
    # gensub with first match only
    print gensub(/[aeiou]/, "X", 1, "hello world")
    # gensub with count
    print gensub(/./, "X", 3, "abcdef")
    # gensub no match
    print gensub(/zzz/, "X", "g", "hello")
}
