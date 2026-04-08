# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:54:35
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **mawk:** `/opt/homebrew/bin/mawk` (`mawk 1.3.4`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 75.8 ± 7.8 | 69.5 | 95.3 | 9.44 ± 1.26 |
| `gawk` | 25.2 ± 1.2 | 24.0 | 28.4 | 3.14 ± 0.31 |
| `mawk` | 17.2 ± 0.6 | 16.4 | 18.8 | 2.14 ± 0.20 |
| `awkrs -j1` | 8.0 ± 0.7 | 7.5 | 9.6 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.5 ± 0.8 | 13.2 | 16.5 | 1.45 ± 0.11 |
| `gawk` | 19.5 ± 0.8 | 17.9 | 21.2 | 1.95 ± 0.12 |
| `mawk` | 10.0 ± 0.5 | 9.5 | 11.1 | 1.00 |
| `awkrs` | 10.9 ± 0.5 | 9.9 | 12.0 | 1.09 ± 0.07 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 66.6 ± 5.3 | 61.6 | 82.1 | 7.60 ± 0.68 |
| `gawk` | 17.5 ± 0.7 | 16.6 | 18.9 | 2.00 ± 0.11 |
| `mawk` | 11.4 ± 0.6 | 10.4 | 12.4 | 1.31 ± 0.08 |
| `awkrs -j1` | 8.8 ± 0.4 | 7.9 | 9.3 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
