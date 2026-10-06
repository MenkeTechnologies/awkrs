
# portable:3010 — a `return` from inside `for (k in a)` does not disturb the
# caller's own `for (k in …)` loop.
function any_key(arr,   k) { for (k in arr) return 1; return 0 }
function count_in(arr,   k, n) { for (k in arr) { n++; if (n == 2) return n } return n }
BEGIN {
    inner["x"]; inner["y"]; inner["z"]
    outer[1]; outer[2]; outer[3]
    for (j in outer) {
        steps++
        hits += any_key(inner) + (count_in(inner) == 2)
        if (steps > 10) break
    }
    print steps, hits
}
