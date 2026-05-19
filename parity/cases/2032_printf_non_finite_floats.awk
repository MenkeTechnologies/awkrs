# gawk parity for non-finite values across %g/%G/%f/%F/%e/%E/%a/%A.
BEGIN {
    pinf = exp(800)
    ninf = -exp(800)

    printf "%g %G %f %F %e %E %a %A\n", pinf, pinf, pinf, pinf, pinf, pinf, pinf, pinf
    printf "%g %G %f %F %e %E %a %A\n", ninf, ninf, ninf, ninf, ninf, ninf, ninf, ninf

    # Width padding for non-finite values uses spaces (zero-pad is ignored).
    printf "[%10g] [%-10g|]\n", pinf, pinf
    printf "[%010f]\n",         pinf
}
