# portable:3014 — `$` binds tighter than postfix `++`/`--`: `$i++` is `($i)++`,
# incrementing the field and leaving `i` alone; `$++i` still pre-increments
# the index.
{
    i = 1
    print $i++, i
    print $++i
    $2++
    print
    j = 1
    x = $j--
    print x, j, $0
    y = $NF++
    print y, $0
    n = 3
    while ($n-- > 7)
        print n, $0
}
