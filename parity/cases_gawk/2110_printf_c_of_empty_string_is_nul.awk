# gawk (and mawk): `%c` of an empty string emits one NUL byte — the string's
# terminator — padded like any other character. one-true-awk emits nothing.
BEGIN {
    printf "[%c][%3c][%-2c]\n", "", "", ""
    x = sprintf("%c", ""); print length(x), (x == "")
    e = ""; printf "%c|\n", e
}
