# portable:3017 — a newline may follow the `,` of a range pattern or of a
# function's parameter list, and may separate a function header's `)` from
# the `{` of its body.
function add(a,
             b)
{
    return a + b
}
/one/,
/two/ { print "r1:", $0 }
NR == 2,
NR == 3 { print "r2:", add(NR,
                             10) }
