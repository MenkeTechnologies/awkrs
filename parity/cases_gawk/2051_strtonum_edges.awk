# gawk parity for strtonum quirks:
#   - longest leading numeric prefix (so "42abc" → 42, not 0)
#   - bare "inf"/"nan" → 0 (no digit/sign prefix)
#   - signed "+inf"/"-inf" / "+nan" / "-nan" → the matching non-finite value
#   - `0x…` hex form is only honored when unsigned
BEGIN {
    print strtonum("42abc")
    print strtonum("  -5.5xyz  ")
    print strtonum("nan")
    print strtonum("inf")
    print strtonum("+inf")
    print strtonum("-inf")
    print strtonum("0x10")
    print strtonum("+0x10")
    print strtonum("-0x10")
    print strtonum("010")     # octal
    print strtonum("1e3foo")
}
