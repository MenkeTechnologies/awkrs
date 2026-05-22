# With `-M`, `print` of an integer-valued bignum must show all digits exactly,
# not OFMT-truncate to scientific form. The fast path in `mpfr_to_string_ofmt`
# / `mpfr_to_string_convfmt` checks `Float::is_integer()` before falling back
# to OFMT, matching gawk parity.
BEGIN {
    print 2^100
    print 2^200

    # Non-integer bignums still honor OFMT (`%.6g` by default).
    print 1/3
    print 22/7
}
