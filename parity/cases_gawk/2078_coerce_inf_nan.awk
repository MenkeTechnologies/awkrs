# gawk parity: bare "inf"/"nan"/"infinity" coerce to 0 (gawk's numeric scan
# rejects non-digit, non-sign prefixes). Sign-prefixed three-letter "inf"/"nan"
# are accepted (case-insensitive); longer forms like "+infinity" are NOT.
BEGIN {
    print +"inf", +"nan", +"infinity", +"NaN"
    print +"+inf", +"-inf", +"+nan", +"-nan"
    print +"+infinity", +"-infinity", +"+Infinity"
    print +"+INF", +"+InF", +"-NAN"
    print +"+infzzz", +"+infabc"
    # Number prefixes still work even with trailing garbage.
    print +"42abc", +"3.14xyz", +"1e10x"
    # Overflow → inf (no sign-name keyword, just a digit-overflow).
    print +"1e500", +"-1e500"
}
