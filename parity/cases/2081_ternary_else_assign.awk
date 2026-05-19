# gawk parity: the else-branch of `? :` is an assignment-expression — so
# `(1 ? x=1 : x=2)` parses with `x=2` as the else arm. Earlier awkrs only
# allowed `conditional_expression` there and emitted a parse error.
BEGIN {
    x=0; (1 ? x=1 : x=2); print x
    y=0; (0 ? y=1 : y=2); print y
    # nested ternary still works on the else side
    z = 1 ? "a" : 2 ? "b" : "c"
    print z
    # Cond ? assign : assign with side effects
    a=0; b=0; (1 ? a=10 : b=20); print a, b
    (0 ? a=10 : b=20); print a, b
}
