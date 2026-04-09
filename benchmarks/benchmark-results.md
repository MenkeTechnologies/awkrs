# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-09 06:37:52
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.3`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 39.3 ± 3.4 | 35.3 | 51.8 | 10.07 ± 1.26 |
| `gawk` | 24.8 ± 2.4 | 22.5 | 37.2 | 6.34 ± 0.85 |
| `awkrs` | 3.9 ± 0.4 | 3.2 | 5.5 | 1.00 |
| `awkrs (parallel)` | 105.0 ± 1.8 | 101.7 | 108.5 | 26.87 ± 2.49 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.8 ± 1.0 | 12.3 | 19.2 | 2.83 ± 0.30 |
| `gawk` | 19.3 ± 1.1 | 16.7 | 23.2 | 3.69 ± 0.37 |
| `awkrs` | 5.2 ± 0.4 | 4.5 | 6.5 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 31.0 ± 2.8 | 27.8 | 40.1 | 5.45 ± 0.70 |
| `gawk` | 15.9 ± 0.7 | 14.5 | 18.2 | 2.79 ± 0.28 |
| `awkrs` | 5.7 ± 0.5 | 4.6 | 7.4 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

