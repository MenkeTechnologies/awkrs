# FIELDWIDTHS supports gawk's `skip:width` (skip N bytes before the field) and
# `*` (take everything remaining). The last entry is clamped to its declared
# width — it does NOT auto-extend to the end of the record.
BEGIN { FIELDWIDTHS = "3 2:2 *" }
{
    printf "NF=%d [%s][%s][%s]\n", NF, $1, $2, $3
}
