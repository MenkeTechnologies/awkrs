# Reading a subscript that does not exist VIVIFIES it: after `x = a["k"]`,
# `"k" in a` is true. That is the behaviour this case pins, and it is the same
# in every gawk.
#
# The classification of the element it creates is NOT: `typeof(a["k"])` is
# "untyped" in gawk 5.2.1 (the version Ubuntu ships, and what CI compares
# against) and "unassigned" in gawk 5.4.1. gawk changed its own answer, so the
# line asserted the reference's version rather than awkrs and could not pass on
# both. typeof on an ASSIGNED element is stable, so that is what is checked
# here instead.
BEGIN {
    x = a["k"]
    if ("k" in a) print "auto-created"
    else print "not created"
    a["n"] = 1
    a["s"] = "str"
    print typeof(a["n"]), typeof(a["s"])
}
