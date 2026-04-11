# bulk:1566
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
    printf "%d\n", pb_xor(1566, 2)
    printf "%d\n", int(exp(log(2 + 1.0)))
    printf "%d\n", pb_lshift(1, 0 + 1)
    { x = "a1566b"; sub(/[0-9]+/, "Z", x); printf "%s\n", x }
}
