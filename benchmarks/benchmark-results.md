# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-07 14:18:13
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 42.5 ± 4.3 | 38.2 | 59.8 | 1.72 ± 0.21 |
| `gawk` | 24.8 ± 1.7 | 21.0 | 31.3 | 1.00 |
| `awkrs -j1` | 128.8 ± 4.0 | 122.3 | 137.5 | 5.20 ± 0.38 |
| `awkrs (parallel)` | 109.3 ± 1.5 | 106.9 | 113.3 | 4.42 ± 0.30 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.3 ± 0.7 | 13.3 | 16.9 | 1.00 |
| `gawk` | 20.1 ± 0.7 | 18.5 | 22.0 | 1.32 ± 0.07 |
| `awkrs` | 70.9 ± 0.7 | 70.0 | 73.2 | 4.65 ± 0.21 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 33.2 ± 2.9 | 30.4 | 47.7 | 1.68 ± 0.66 |
| `gawk` | 19.7 ± 7.6 | 16.0 | 62.0 | 1.00 |
| `awkrs -j1` | 47.1 ± 6.9 | 40.6 | 83.1 | 2.39 ± 0.98 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

