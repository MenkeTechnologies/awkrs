# gawk's fw_parse_field returns no fields for an empty record, so NF is 0
# there as it is for FS splitting.
BEGIN { FIELDWIDTHS = "2 3:1 *" }
{ printf "%d [%s][%s][%s]\n", NF, $1, $2, $3 }
END { $0 = ""; print NF; $0 = "a"; print NF }
