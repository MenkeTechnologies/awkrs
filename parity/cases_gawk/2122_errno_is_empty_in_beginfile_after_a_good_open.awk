# gawk opens each operand before BEGINFILE: a good open leaves ERRNO empty
# there even when an earlier getline failed, while getline failures set it.
BEGIN { getline x < "/nonexistent/awkrs-parity"; print "begin [" ERRNO "]" }
BEGINFILE { print "beginfile [" ERRNO "]" }
{ print NR, $0 }
