# gawk parity: when comparing a Num to a non-numeric-string (e.g. a string
# literal), the string-compare fallback stringifies the Num via CONVFMT, not
# via the default `%.6g`. Before the fix, awkrs treated string literals as
# numeric strings for this path and produced numeric-compare results.
BEGIN {
    CONVFMT = "%.2f"
    x = 3.14159

    # Equality with a string literal: gawk stringifies x via CONVFMT → "3.14"
    print (x == "3.14")        # 1
    print (x == "3.14159")     # 0 (CONVFMT-stringified "3.14" != "3.14159")
    print (x != "3.14")        # 0
    print (x "" == "3.14")     # 1 (explicit concat coerces via CONVFMT)

    # Relational compare uses the same string-compare fallback.
    print (x < "3.2")          # 1 ("3.14" < "3.2")
    print (x > "3.0")          # 1 ("3.14" > "3.0")

    # Integer-valued bypass CONVFMT — y stringifies as "42" regardless.
    CONVFMT = "%.6f"
    y = 42
    print (y == "42")          # 1
    print (y == "42.000000")   # 0
}
