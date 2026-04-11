# portable:2843
BEGIN {
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 3, 4)
    { _s = ""; for (_i = 0; _i < 3 + 2; _i++) _s = _s "0"; printf "%d\n", length(_s) }
    { _v = 2; printf "%d\n", _v ? 5 : 0 }
}
