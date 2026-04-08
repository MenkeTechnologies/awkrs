# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:46:42
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 44.1 ± 4.8 | 37.2 | 62.6 | 1.81 ± 0.30 |
| `gawk` | 24.4 ± 3.0 | 20.7 | 39.2 | 1.00 |
| `awkrs -j1` | 37.3 ± 6.4 | 30.4 | 60.9 | 1.53 ± 0.32 |
| `awkrs (parallel)` | 109.5 ± 2.9 | 105.8 | 116.0 | 4.48 ± 0.56 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 18.2 ± 4.5 | 13.5 | 44.3 | 1.05 ± 0.26 |
| `gawk` | 23.9 ± 2.3 | 20.5 | 33.8 | 1.38 ± 0.15 |
| `awkrs` | 17.3 ± 0.7 | 15.5 | 19.6 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 32.8 ± 4.9 | 28.6 | 55.5 | 1.97 ± 0.31 |
| `gawk` | 16.7 ± 0.9 | 15.0 | 20.3 | 1.00 |
| `awkrs -j1` | 35.8 ± 5.4 | 23.1 | 53.1 | 2.15 ± 0.35 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

