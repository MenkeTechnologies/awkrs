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
| `BSD awk` | 39.3 ± 2.5 | 36.1 | 47.4 | 1.58 ± 0.13 |
| `gawk` | 24.9 ± 1.3 | 22.7 | 28.4 | 1.00 |
| `awkrs -j1` | 25.3 ± 1.1 | 23.5 | 29.2 | 1.02 ± 0.07 |
| `awkrs (parallel)` | 110.6 ± 2.8 | 105.7 | 116.8 | 4.45 ± 0.25 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.4 ± 0.9 | 13.1 | 18.7 | 1.00 |
| `gawk` | 20.5 ± 1.0 | 18.3 | 24.1 | 1.33 ± 0.10 |
| `awkrs` | 18.2 ± 0.7 | 17.0 | 21.6 | 1.18 ± 0.08 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 33.2 ± 2.4 | 29.7 | 40.4 | 1.94 ± 0.18 |
| `gawk` | 18.1 ± 0.9 | 16.3 | 21.4 | 1.05 ± 0.08 |
| `awkrs -j1` | 17.2 ± 1.0 | 15.5 | 20.3 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

