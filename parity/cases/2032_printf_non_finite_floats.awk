# gawk parity for non-finite values across %g/%G/%f/%F/%e/%E/%a/%A.
# (Width-padding behavior on non-finite values diverges between gawk
#  versions before/after 5.3, so this case omits width specifiers.
#  Inf is constructed via string coercion to avoid `exp(800)` warning
#  on older gawk.)
BEGIN {
    pinf = "+inf"+0
    ninf = "-inf"+0

    printf "%g %G %f %F %e %E %a %A\n", pinf, pinf, pinf, pinf, pinf, pinf, pinf, pinf
    printf "%g %G %f %F %e %E %a %A\n", ninf, ninf, ninf, ninf, ninf, ninf, ninf, ninf
}
