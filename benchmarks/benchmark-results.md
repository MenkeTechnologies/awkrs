# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:29:18
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 38.3 ± 1.7 | 35.8 | 43.4 | 1.53 ± 0.14 |
| `gawk` | 25.1 ± 2.0 | 22.1 | 32.8 | 1.00 |
| `awkrs -j1` | 145.0 ± 4.6 | 135.1 | 150.8 | 5.79 ± 0.50 |
| `awkrs (parallel)` | 115.6 ± 2.4 | 112.3 | 121.5 | 4.62 ± 0.38 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.9 ± 0.8 | 13.1 | 17.5 | 1.00 |
| `gawk` | 20.3 ± 1.1 | 18.0 | 22.9 | 1.36 ± 0.10 |
| `awkrs` | 15.8 ± 0.4 | 15.1 | 17.8 | 1.06 ± 0.06 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 32.3 ± 3.3 | 27.1 | 43.9 | 1.88 ± 0.24 |
| `gawk` | 17.1 ± 1.3 | 14.5 | 20.2 | 1.00 |
| `awkrs -j1` | 42.8 ± 2.7 | 37.2 | 49.5 | 2.50 ± 0.25 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

