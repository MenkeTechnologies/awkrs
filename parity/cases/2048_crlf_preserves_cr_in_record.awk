# gawk parity (Unix text mode): only `\n` is the record terminator. A trailing
# `\r` from CRLF input stays in `$0` and contributes to `length`. awkrs used to
# strip both `\n` and `\r`, silently dropping CR bytes.
{
    printf "%d|%d|", NR, length
    # Print `[content]` with a `<CR>` marker if a CR is present so the diff is
    # unambiguous (the `\r` would otherwise overprint the closing bracket on
    # some terminals when looking at raw bytes).
    if (index($0, "\r") > 0) printf "[CR]\n"
    else printf "[%s]\n", $0
}
