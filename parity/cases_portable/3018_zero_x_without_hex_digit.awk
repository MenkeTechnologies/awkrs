# portable:3018 — `0x` not followed by a hex digit is the number 0 and then a
# name starting with `x` (gawk pushes the `x` back; one-true-awk never reads
# hex), so `0x` and `0xg` concatenate 0 with a variable instead of failing to
# parse.
BEGIN {
    x = "X"; xg = "G"
    print 0x, 0xg
    print 0x "" 1
}
