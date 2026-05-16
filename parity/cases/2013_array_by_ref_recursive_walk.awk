function walk(a, n,    i) {
    if (n <= 0) return
    a[n] = n * n
    walk(a, n - 1)
}
BEGIN {
    walk(arr, 5)
    for (i = 1; i <= 5; i++) print i, arr[i]
}
