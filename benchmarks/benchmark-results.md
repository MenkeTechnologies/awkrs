# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:54:35
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 75.6 ± 7.9 | 70.2 | 103.2 | 8.54 ± 0.95 |
| `gawk` | 27.1 ± 2.4 | 24.0 | 34.1 | 3.06 ± 0.29 |
| `awkrs -j1` | 8.9 ± 0.3 | 8.1 | 9.3 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.5 ± 1.1 | 13.0 | 16.4 | 1.45 ± 0.12 |
| `gawk` | 19.1 ± 1.1 | 17.1 | 20.7 | 1.91 ± 0.12 |
| `awkrs` | 10.0 ± 0.3 | 9.5 | 10.6 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 68.1 ± 10.7 | 62.8 | 110.4 | 7.13 ± 1.17 |
| `gawk` | 18.2 ± 0.8 | 16.9 | 20.4 | 1.90 ± 0.12 |
| `awkrs -j1` | 9.5 ± 0.4 | 8.8 | 10.7 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
