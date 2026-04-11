# portable:2610
BEGIN {
    { x = "n2610n"; gsub(/n/, "m", x); printf "%s\n", x }
    printf "%d\n", (atan2(1, 1) > 0)
    printf "%s\n", substr("0123456789", 0, 4)
    { _s = ""; for (_i = 0; _i < 0 + 2; _i++) _s = _s "0"; printf "%d\n", length(_s) }
}
