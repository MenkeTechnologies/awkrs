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
| `BSD awk` | 80.4 ± 11.9 | 70.1 | 109.2 | 9.87 ± 1.50 |
| `gawk` | 27.1 ± 1.9 | 25.0 | 32.1 | 3.32 ± 0.26 |
| `mawk` | 19.2 ± 1.9 | 16.8 | 22.8 | 2.36 ± 0.25 |
| `awkrs -j1` | 8.1 ± 0.3 | 7.8 | 8.8 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.3 ± 0.7 | 13.1 | 16.0 | 2.66 ± 0.28 |
| `gawk` | 18.9 ± 0.8 | 17.1 | 20.2 | 3.52 ± 0.36 |
| `mawk` | 8.4 ± 0.5 | 7.4 | 9.3 | 1.57 ± 0.17 |
| `awkrs` | 5.4 ± 0.5 | 4.7 | 7.3 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 68.9 ± 7.1 | 61.8 | 84.2 | 7.05 ± 1.66 |
| `gawk` | 16.9 ± 0.9 | 15.3 | 18.3 | 1.73 ± 0.38 |
| `mawk` | 15.4 ± 13.1 | 10.6 | 67.5 | 1.58 ± 1.38 |
| `awkrs -j1` | 9.8 ± 2.1 | 8.6 | 17.6 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
