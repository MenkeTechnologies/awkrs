
# portable:3004 — the redirected and piped `getline` forms.
# `getline < file` sets $0 and NF but NOT NR/FNR; `getline var < file` sets
# neither. An unreadable source is -1, distinct from the 0 that means end of
# input, and neither is fatal.
BEGIN {
    f = ARGV[1]
    while ((r = (getline < f)) > 0) {
        printf "file  : ret=%d NR=%d FNR=%d NF=%d $0=[%s]\n", r, NR, FNR, NF, $0
    }
    printf "eof   : ret=%d\n", r
    close(f)
    r = (getline v < f)
    printf "fvar  : ret=%d NR=%d NF=%d $0=[%s] v=[%s]\n", r, NR, NF, $0, v
    close(f)
    printf "absent: ret=%d\n", (getline z < "/nonexistent/awkrs/parity/source")
    # `cmd | getline` sets $0 and NF; `cmd | getline var` sets only the variable.
    while (("echo p1 p2; echo p3 p4" | getline) > 0) {
        printf "cmd   : NF=%d $0=[%s]\n", NF, $0
    }
    close("echo p1 p2; echo p3 p4")
    "echo q1 q2" | getline w
    printf "cmdvar: NF=%d $0=[%s] w=[%s]\n", NF, $0, w
    close("echo q1 q2")
    # ARGV[1] is consumed as a file below unless it is cleared.
    ARGV[1] = ""
}
