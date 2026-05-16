function copy(src, dst,    k) { for (k in src) dst[k] = src[k] }
BEGIN {
    a[1] = "x"
    a[2] = "y"
    a[3] = "z"
    copy(a, b)
    print b[1], b[2], b[3]
}
