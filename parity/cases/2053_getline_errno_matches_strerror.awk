# gawk parity: when `getline < file` fails because the file doesn't exist,
# ERRNO contains the strerror message ("No such file or directory") — NOT the
# Rust-style "(os error N)" suffix.
BEGIN {
    r = (getline line < "/tmp/awkrs_definitely_does_not_exist_zzz_xyz123")
    print r
    print "[" ERRNO "]"
}
