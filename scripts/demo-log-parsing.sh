#!/usr/bin/env bash
# Applied demo — log parsing, ETL, and report generation with awkrs.
# Generates synthetic input files in $TMPDIR and runs four small pipelines:
#   1. Apache-style access log → status code distribution
#   2. CSV time-series → per-key min/max/mean
#   3. ps-style output → top-3 RSS consumers
#   4. /etc/passwd-style → shell histogram
# Cleans up its temp files on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
builtin cd "$ROOT" || exit 1

AWKRS="$ROOT/target/debug/awkrs"
if [[ ! -x "$AWKRS" ]]; then
  echo "Building awkrs (debug)..."
  command cargo build -q
fi

if [[ -n "${NO_COLOR:-}" ]] || [[ ! -t 1 ]]; then
  HDR=""; OFF=""
else
  HDR=$'\033[1;36m'; OFF=$'\033[0m'
fi

TMP_DIR="$(command mktemp -d "${TMPDIR:-/tmp}/awkrs-demo.XXXXXX")"
trap 'command rm -rf "$TMP_DIR"' EXIT

ACCESS="$TMP_DIR/access.log"
METRICS="$TMP_DIR/metrics.csv"
PSOUT="$TMP_DIR/ps.txt"
PASSWD="$TMP_DIR/passwd.txt"

# ── 1. Apache-style access log ──────────────────────────────────────────────
command cat >"$ACCESS" <<'EOF'
10.0.0.1 - - [10/Apr/2026:10:00:01 +0000] "GET /index HTTP/1.1" 200 1024
10.0.0.2 - - [10/Apr/2026:10:00:02 +0000] "GET /api/user HTTP/1.1" 200 512
10.0.0.3 - - [10/Apr/2026:10:00:03 +0000] "POST /api/login HTTP/1.1" 401 64
10.0.0.4 - - [10/Apr/2026:10:00:04 +0000] "GET /missing HTTP/1.1" 404 0
10.0.0.5 - - [10/Apr/2026:10:00:05 +0000] "GET /index HTTP/1.1" 200 1024
10.0.0.1 - - [10/Apr/2026:10:00:06 +0000] "GET /api/user HTTP/1.1" 500 0
10.0.0.6 - - [10/Apr/2026:10:00:07 +0000] "GET /healthz HTTP/1.1" 200 4
10.0.0.7 - - [10/Apr/2026:10:00:08 +0000] "GET /api/user HTTP/1.1" 200 512
10.0.0.8 - - [10/Apr/2026:10:00:09 +0000] "POST /api/login HTTP/1.1" 200 256
10.0.0.9 - - [10/Apr/2026:10:00:10 +0000] "GET /missing HTTP/1.1" 404 0
EOF

command printf '\n%s[1] Access log → status distribution + top IPs%s\n' "$HDR" "$OFF"
"$AWKRS" '
{
  status[$(NF - 1)]++
  ip[$1]++
  bytes += $NF
}
END {
  print "── Status codes ──"
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (s in status) printf "  %s: %d\n", s, status[s]
  print "── Top IPs ──"
  PROCINFO["sorted_in"] = "@val_num_desc"
  n = 0
  for (i in ip) {
    if (++n > 3) break
    printf "  %-12s %d req\n", i, ip[i]
  }
  printf "── Total bytes: %d ──\n", bytes
}' "$ACCESS"

# ── 2. CSV time-series ──────────────────────────────────────────────────────
command cat >"$METRICS" <<'EOF'
timestamp,host,cpu,mem
1681120800,host-a,42.5,1024
1681120860,host-b,15.0,512
1681120920,host-a,58.3,1100
1681120980,host-c,90.1,2048
1681121040,host-b,22.7,520
1681121100,host-a,61.0,1150
1681121160,host-c,88.5,2100
1681121220,host-b,18.4,505
EOF

command printf '\n%s[2] CSV metrics → per-host min/max/mean CPU%s\n' "$HDR" "$OFF"
"$AWKRS" -k '
NR == 1 { next }
{
  h = $2; v = $3 + 0
  count[h]++
  sum[h] += v
  if (!(h in min) || v < min[h]) min[h] = v
  if (!(h in max) || v > max[h]) max[h] = v
}
END {
  printf "  %-8s %6s %6s %6s %6s\n", "host", "n", "min", "max", "mean"
  PROCINFO["sorted_in"] = "@ind_str_asc"
  for (h in count) printf "  %-8s %6d %6.1f %6.1f %6.2f\n", h, count[h], min[h], max[h], sum[h] / count[h]
}' "$METRICS"

# ── 3. ps-style output ─────────────────────────────────────────────────────
command cat >"$PSOUT" <<'EOF'
  PID  PPID   RSS COMMAND
  101     1 12000 systemd
  402     1 88000 chrome
  403   402 75000 chrome-renderer
  410   402 64000 chrome-renderer
  511     1 32000 node
  720   511 18000 npm
 1042     1 51000 firefox
 1101  1042 47000 firefox-content
EOF

command printf '\n%s[3] ps output → top 3 by RSS%s\n' "$HDR" "$OFF"
"$AWKRS" '
NR == 1 { next }
{ rss[$NF] += $(NF - 1) }
END {
  PROCINFO["sorted_in"] = "@val_num_desc"
  n = 0
  for (k in rss) {
    if (++n > 3) break
    printf "  %-20s %8d KB\n", k, rss[k]
  }
}' "$PSOUT"

# ── 4. /etc/passwd-style ───────────────────────────────────────────────────
command cat >"$PASSWD" <<'EOF'
root:x:0:0:root:/root:/bin/bash
sshd:x:74:74:Privilege-separated SSH:/run/sshd:/sbin/nologin
mail:x:8:12:mail:/var/mail:/sbin/nologin
alice:x:1000:1000:Alice:/home/alice:/bin/zsh
bob:x:1001:1001:Bob:/home/bob:/bin/bash
carol:x:1002:1002:Carol:/home/carol:/bin/fish
dave:x:1003:1003:Dave:/home/dave:/bin/zsh
nobody:x:65534:65534:Nobody:/:/usr/sbin/nologin
EOF

command printf '\n%s[4] /etc/passwd-style → shell histogram%s\n' "$HDR" "$OFF"
"$AWKRS" -F: '
{ shell[$7]++ }
END {
  PROCINFO["sorted_in"] = "@val_num_desc"
  for (s in shell) {
    bar = ""
    for (i = 0; i < shell[s]; i++) bar = bar "█"
    printf "  %-20s %2d %s\n", s, shell[s], bar
  }
}' "$PASSWD"

command printf '\n%sDone.%s\n' "$HDR" "$OFF"
