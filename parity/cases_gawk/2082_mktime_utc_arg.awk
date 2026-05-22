# gawk parity: `mktime(spec, utc)` — with truthy `utc`, interprets the
# whitespace-separated `YYYY MM DD HH MM SS` datespec in UTC. The local-time
# form (1 arg) still works and depends on $TZ.
BEGIN {
    # UTC-based: epoch for 2024-01-15 12:30:00 UTC = 1705321800
    print mktime("2024 01 15 12 30 00", 1)
    # epoch for 1970-01-01 00:00:00 UTC = 0
    print mktime("1970 01 01 00 00 00", 1)
    # bad datespec still returns -1 with the utc flag
    print mktime("not a date", 1)
}
