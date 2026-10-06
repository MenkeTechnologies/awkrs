# gawk: whichever of FS, FIELDWIDTHS and FPAT was assigned last splits the
# records (set_parser), and PROCINFO["FS"] names it, even for an empty
# FIELDWIDTHS.
# Changing the rule mid-record leaves the current record's fields alone.
NR == 1 { FIELDWIDTHS = "1 1"; FS = ","; print PROCINFO["FS"]; print $1 }
NR == 2 { print $1, NF; FPAT = "[a-z]+"; FIELDWIDTHS = "2 1"; print PROCINFO["FS"], $1 }
NR == 3 { print $1, NF; FIELDWIDTHS = "2"; FPAT = "[a-z]+"; print PROCINFO["FS"] }
NR == 4 { print $1, NF; FIELDWIDTHS = ""; FS = FS; print PROCINFO["FS"] }
NR == 5 { print $1, NF; FIELDWIDTHS = ""; print PROCINFO["FS"] }
