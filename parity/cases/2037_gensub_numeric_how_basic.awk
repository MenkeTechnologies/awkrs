# gawk gensub: positive integer `how` replaces only that occurrence; "g" replaces all.
# (gawk also accepts 0/negative — silently treated as 1 with a warning to stderr —
# but the warning text embeds the source location and is left out of this parity case.)
BEGIN {
    print gensub(/a/, "X", 1,  "aaaa")
    print gensub(/a/, "X", 2,  "aaaa")
    print gensub(/a/, "X", 4,  "aaaa")
    print gensub(/a/, "X", "g", "aaaa")
}
