# gawk parity: `printf "%s"` of a numeric value stringifies via CONVFMT, not
# via the f64 default. Other conversions (%d, %f, %g, %e) use their own
# semantics and ignore CONVFMT.
BEGIN {
    CONVFMT = "%.3f"
    printf "s=%s\n",   3.14159   # CONVFMT-formatted
    printf "d=%d\n",   3.14159   # truncates to integer
    printf "f=%f\n",   3.14159   # default %f precision
    printf "g=%g\n",   3.14159   # default %g precision

    # Multiple args
    printf "%s,%s,%s\n", 1.5, 2.5, 3.5

    # Integer-valued bypass CONVFMT (no precision loss).
    printf "%s\n", 42
    printf "%s\n", 1000000

    CONVFMT = "%.6g"
    printf "%s\n", 3.14159
}
