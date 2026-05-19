# gawk: `switch` arms fall through to the next arm's body until `break`
# (C semantics). Verifies the full matrix: match + fallthrough, match + break,
# middle match, no match with default, no match without default.
BEGIN {
    # Match first arm, fall through to all subsequent arms.
    print "--- fallthrough from a ---"
    x = "a"
    switch (x) {
        case "a": print "A"
        case "b": print "B"
        case "c": print "C"
    }

    # Match with explicit break stops at the matching arm.
    print "--- break after a ---"
    switch ("a") {
        case "a": print "A"; break
        case "b": print "B"
    }

    # Match middle arm.
    print "--- match b ---"
    switch ("b") {
        case "a": print "A"
        case "b": print "B"
        case "c": print "C"
        default:  print "D"
    }

    # No match with default.
    print "--- no match, default ---"
    switch ("z") {
        case "a": print "A"; break
        default:  print "D"
    }

    # No match without default.
    print "--- no match, no default ---"
    switch ("z") {
        case "a": print "A"
        case "b": print "B"
    }
    print "after"
}
