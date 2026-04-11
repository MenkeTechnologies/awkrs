# bulk:1719
function pb_and(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if (ai && bi) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_or(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if (ai || bi) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_xor(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if ((ai && !bi) || (!ai && bi)) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_lshift(x, n,    i) {
    for (i = 0; i < n; i++) {
        x *= 2
    }
    return x
}
function pb_rshift(x, n,    i) {
    for (i = 0; i < n; i++) {
        x = int(x / 2)
    }
    return x
}
BEGIN {
    printf "%d\n", index("alphabet", "h") + 0
    { split("", a2); a2["k"] = 14; printf "%d\n", a2["k"] }
    printf "%d\n", int(sqrt(14 * 14 + 6))
    printf "%d\n", int((6 + 10) * 14 / 7)
}
