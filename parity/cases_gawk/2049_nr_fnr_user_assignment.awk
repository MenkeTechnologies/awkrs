# gawk parity: NR and FNR are user-assignable. The internal record counter is
# updated in lockstep so subsequent records see the user-set value + 1.
BEGIN { NR = 100; FNR = 50 }
{ print NR, FNR, $0 }
