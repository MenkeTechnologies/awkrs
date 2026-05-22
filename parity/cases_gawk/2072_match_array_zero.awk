# gawk extension: `match(s, re, arr)` populates `arr[0]` with the entire
# match and `arr[1]..arr[n]` with the capture groups. Previously awkrs
# skipped `arr[0]` (only filled the per-capture entries).
BEGIN {
    if (match("hello world", /(\w+) (\w+)/, m))
        printf "[%s] [%s] [%s]\n", m[0], m[1], m[2]

    if (match("abc", /(a)|(b)/, alt))
        printf "[%s] [%s] [%s]\n", alt[0], alt[1], alt[2]
}
