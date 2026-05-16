function fill(a, n,    i) {
    for (i = 1; i <= n; i++) a[i] = i * 10
}
BEGIN {
    fill(arr, 3)
    print arr[1], arr[2], arr[3]
}
