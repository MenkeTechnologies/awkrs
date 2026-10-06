# gawk's re_parse_field: a regex FS (or split() separator) match of the empty
# string separates nothing — the scan steps one character on and searches
# again — and when the last search fails the remaining field starts at the
# scan point, which is why FS = "^x*" makes $1 of "abc" "bc".
BEGIN {
    FS = "a*"; $0 = "baaac"; print NF, $1, $2
    FS = "b*"; $0 = "abc"; print NF, $1, $2
    FS = "c*"; $0 = "abc"; print NF "[" $1 "][" $2 "]"
    FS = "x*"; $0 = "abc"; print NF, $1
    FS = "x*$"; $0 = "abc"; print NF, $1
    FS = "^x*"; $0 = "abc"; print NF, $1
    n = split("abc", a, /^x*/); print n, a[1]
    n = split("baaac", a, /a*/, s); print n, a[1], a[2], s[1]
}
