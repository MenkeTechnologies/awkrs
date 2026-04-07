# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-07 14:11:42
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 42.7 ± 4.9 | 37.7 | 67.9 | 1.67 ± 0.33 |
| `gawk` | 25.5 ± 4.2 | 22.2 | 57.7 | 1.00 |
| `awkrs -j1` | 148.5 ± 13.1 | 133.1 | 182.4 | 5.81 ± 1.08 |
| `awkrs (parallel)` | 127.7 ± 20.8 | 107.0 | 179.4 | 5.00 ± 1.15 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 17.3 ± 5.2 | 13.0 | 46.6 | 1.00 |
| `gawk` | 21.8 ± 7.7 | 17.3 | 55.2 | 1.26 ± 0.59 |
| `awkrs` | 74.5 ± 4.9 | 66.8 | 85.1 | 4.30 ± 1.33 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 34.1 ± 4.0 | 30.9 | 51.9 | 1.75 ± 0.73 |
| `gawk` | 19.5 ± 7.8 | 15.7 | 59.5 | 1.00 |
| `awkrs -j1` | 44.5 ± 6.2 | 37.8 | 66.0 | 2.28 ± 0.97 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

