# portable:3012 — with `#` and the `0` flag, the zeros that fill the width go
# between the `0x`/`0X` prefix and the digits, as for a sign.
BEGIN {
    printf "[%#05x] [%#08X] [%#010x] [%#-08x] [%#08o] [%#06x]\n", 1, 255, 48879, 255, 8, 0
    printf "[%#010.3x] [%08.3x] [%#3x] [%#05X]\n", 5, 5, 255, 3054
}
