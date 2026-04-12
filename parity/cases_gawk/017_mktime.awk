BEGIN {
    printf "%d\n", mktime("2024 1 1 0 0 0")
    # mktime returns epoch seconds — verify it's a positive large number
    t = mktime("2024 6 15 12 30 0")
    print (t > 1700000000 ? "ok" : "fail")
}
