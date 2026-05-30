# Date math via Zeller's congruence + a tiny calendar generator.
# Input lines:
#   "DOW <yyyy> <mm> <dd>"       day-of-week:  prints "yyyy-mm-dd is <weekday>"
#   "CAL <yyyy> <mm>"            print a month calendar (Sun–Sat columns)
#   "DIFF <y1> <m1> <d1> AND <y2> <m2> <d2>"
#                                 print "<a> to <b> = <n> days"
# Algorithm: days-from-epoch using a Gregorian formula good for AD 1+.

function is_leap(y) { return ((y % 4 == 0) && (y % 100 != 0)) || (y % 400 == 0) }

function days_in_month(y, m,   d) {
  if (m == 2) return is_leap(y) ? 29 : 28
  if (m == 4 || m == 6 || m == 9 || m == 11) return 30
  return 31
}

function day_of_week(y, m, d,   K, J, h) {
  # Zeller for Gregorian. Output: 0=Sat ... 6=Fri. We rotate to 0=Sun.
  if (m < 3) { m += 12; y-- }
  K = y % 100
  J = int(y / 100)
  h = (d + int(13 * (m + 1) / 5) + K + int(K / 4) + int(J / 4) + 5 * J) % 7
  return (h + 6) % 7   # rotate: 0=Sun, 6=Sat
}

function fixed_from_gregorian(y, m, d,   y0) {
  # Days from 0000-12-31. Used only for differences, so the exact epoch
  # doesn't matter — only deltas are reported.
  y0 = y - 1
  return 365 * y0 + int(y0 / 4) - int(y0 / 100) + int(y0 / 400) \
       + int((367 * m - 362) / 12) \
       + (m <= 2 ? 0 : (is_leap(y) ? -1 : -2)) \
       + d
}

function wd_name(d,   names) {
  split("Sun Mon Tue Wed Thu Fri Sat", names, " ")
  return names[d + 1]
}

function month_name(m,   names) {
  split("January February March April May June July August September October November December", names, " ")
  return names[m]
}

function cal(y, m,   start, n, c, w, row) {
  printf "    %s %d\n", month_name(m), y
  print "Su Mo Tu We Th Fr Sa"
  start = day_of_week(y, m, 1)
  n = days_in_month(y, m)
  row = ""
  for (c = 0; c < start; c++) row = row "   "
  w = start
  for (c = 1; c <= n; c++) {
    row = row sprintf("%2d ", c)
    w++
    if (w % 7 == 0) { sub(/ $/, "", row); print row; row = "" }
  }
  if (row != "") { sub(/ $/, "", row); print row }
}

$1 == "DOW" {
  y = $2 + 0; m = $3 + 0; d = $4 + 0
  printf "%04d-%02d-%02d is %s\n", y, m, d, wd_name(day_of_week(y, m, d))
  next
}
$1 == "CAL" {
  cal($2 + 0, $3 + 0)
  next
}
$1 == "DIFF" {
  y1 = $2 + 0; m1 = $3 + 0; d1 = $4 + 0
  y2 = $6 + 0; m2 = $7 + 0; d2 = $8 + 0
  diff = fixed_from_gregorian(y2, m2, d2) - fixed_from_gregorian(y1, m1, d1)
  if (diff < 0) diff = -diff
  printf "%04d-%02d-%02d to %04d-%02d-%02d = %d days\n", y1, m1, d1, y2, m2, d2, diff
  next
}
