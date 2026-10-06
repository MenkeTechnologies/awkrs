# gawk do_close: closing a name that was never opened returns -1 and sets
# ERRNO to "close of redirection that was never opened".
BEGIN { r = close("never-opened"); print r, "[" ERRNO "]" }
