
# portable:3006 — the FS rules, which are four different rules wearing one name.
BEGIN {
    OFS = ":"
    # Assigning NF truncates or extends, rebuilding $0 with OFS.
    $0 = "a b c d e"
    NF = 3; printf "%d [%s]\n", NF, $0
    NF = 5; printf "%d [%s]\n", NF, $0
    # A single space means "runs of blanks, leading and trailing ignored" —
    # which is NOT what the equivalent-looking regex does.
    FS = " "; $0 = "   x   y   "; printf "%d [%s][%s]\n", NF, $1, $2
    FS = "[ ]"; $0 = "   x   y   "; printf "%d [%s][%s]\n", NF, $1, $2
    # A single character is literal, so a "." separator is not "any character".
    FS = "."; $0 = "a.b.c"; printf "%d [%s]\n", NF, $2
    FS = "|"; $0 = "a|b|c"; printf "%d [%s]\n", NF, $2
    # More than one character is a regex.
    FS = "[0-9]+"; $0 = "a12b345c"; printf "%d [%s][%s]\n", NF, $2, $3
    # split() takes its own separator and disturbs neither FS nor $0.
    FS = ","
    n = split("p-q-r", a1, "-")
    printf "%d [%s] FS=[%s] $0=[%s]\n", n, a1[2], FS, $0
    # An empty separator splits into characters.
    n = split("abc", ch, "")
    printf "%d [%s][%s][%s]\n", n, ch[1], ch[2], ch[3]
}
