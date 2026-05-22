# `print` of an infinity goes through OFMT (`%.6g`), which produces "+inf" — matches gawk.
# `printf "%s"` of the same value produces the same spelling so the two paths agree.
# Inf is constructed via string coercion to avoid `exp(800)` warning on older gawk.
BEGIN {
    x = "+inf"+0
    print x
    printf "%s\n", x
    printf "%g\n", x
    printf "%G %F %E\n", x, x, x
    print -x
}
