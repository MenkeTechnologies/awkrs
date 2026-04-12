BEGIN {
    # mkbool
    print mkbool(1)
    print mkbool(0)
    print mkbool("yes")
    print mkbool("")

    # compl
    # gawk compl works on unsigned 64-bit
    print and(compl(0), 0xFF)
    print and(compl(0xFF), 0xFF)
}
