# Single-char FS in `split` is a literal character — even regex metacharacters
# like `.` are matched literally (not as "any byte").
BEGIN {
    n = split("a.b.c", a, ".")
    print n, a[1], a[2], a[3]

    m = split("1.2.3.4", parts, ".")
    print m, parts[1], parts[2], parts[3], parts[4]
}
