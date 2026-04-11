# portable:2373
BEGIN {
    { _s = ""; for (_i = 0; _i < 3 + 2; _i++) _s = _s "0"; printf "%d\n", length(_s) }
    { _v = 0; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab3c")
    printf "%d\n", int((25 + 57) * 67 / 7)
}
