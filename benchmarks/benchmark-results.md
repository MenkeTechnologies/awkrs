# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-09 06:55:37
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.3`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 41.2 ± 4.0 | 36.1 | 61.3 | 6.67 ± 3.22 |
| `gawk` | 25.1 ± 1.6 | 21.9 | 30.5 | 4.05 ± 1.93 |
| `awkrs` | 6.2 ± 2.9 | 3.4 | 18.9 | 1.00 |
| `awkrs (parallel)` | 115.3 ± 8.2 | 103.8 | 132.7 | 18.66 ± 8.91 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.4 ± 1.1 | 12.6 | 18.5 | 2.91 ± 0.36 |
| `gawk` | 19.8 ± 1.1 | 17.0 | 22.7 | 3.74 ± 0.42 |
| `awkrs` | 5.3 ± 0.5 | 4.5 | 7.1 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 32.1 ± 3.9 | 27.9 | 44.5 | 5.09 ± 1.75 |
| `gawk` | 17.2 ± 2.2 | 14.8 | 32.4 | 2.72 ± 0.94 |
| `awkrs` | 6.3 ± 2.0 | 4.9 | 37.9 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

