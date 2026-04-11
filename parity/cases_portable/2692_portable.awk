# portable:2692
BEGIN {
    printf "%s\n", substr("0123456789", 2, 4)
    { _s = ""; for (_i = 0; _i < 2 + 2; _i++) _s = _s "0"; printf "%d\n", length(_s) }
    { _v = 1; printf "%d\n", _v ? 5 : 0 }
    printf "%s\n", toupper("ab2c")
}
