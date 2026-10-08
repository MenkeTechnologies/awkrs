# gawk:2127 — naming an untyped variable as the target of sub/gsub (no match)
# or of getline (end of input, failed open, empty command output) references
# it, so typeof reports "unassigned", not "untyped"; an element target is
# created. A target that was never named stays "untyped".
BEGIN {
    sub(/x/, "y", s1)
    gsub(/x/, "y", s2)
    getline g1 < "/dev/null"
    getline g2 < "/nonexistent/awkrs-parity-2127"
    "true" | getline g3
    sub(/x/, "y", arr[1])
    print typeof(s1), typeof(s2), typeof(g1), typeof(g2), typeof(g3), typeof(never)
    print typeof(arr[1]), length(arr), (1 in arr)
    n = 5
    sub(/q/, "r", n)
    print typeof(n), n
}
