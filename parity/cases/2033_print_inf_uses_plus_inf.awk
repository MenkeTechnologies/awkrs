# `print` of an infinity goes through OFMT (`%.6g`), which produces "+inf" — matches gawk.
# `printf "%s"` of the same value produces the same spelling so the two paths agree.
# (NaN behavior is identical but constructing a NaN portably requires sqrt(-1) which
# emits a warning to stderr in both awks; this case avoids the warning by using exp.)
BEGIN {
    x = exp(800)
    print x
    printf "%s\n", x
    printf "%g\n", x
    printf "%G %F %E\n", x, x, x
    print -x
}
