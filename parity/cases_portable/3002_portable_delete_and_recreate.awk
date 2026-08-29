# portable:3002 — a deleted subscript can be created again and reads back what
# was stored; `delete` on one element leaves the rest alone.
BEGIN {
    for (i = 1; i <= 4; i++) a[i] = i * 10
    delete a[2]
    printf "%d %s\n", (2 in a), a[3]
    a[2] = 99
    printf "%d %s %s\n", (2 in a), a[2], a[4]
    n = 0
    for (i = 1; i <= 4; i++) if (i in a) n++
    printf "present=%d\n", n
}
