# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:35:35
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 40.3 ± 3.7 | 36.1 | 50.2 | 1.67 ± 0.19 |
| `gawk` | 24.2 ± 1.5 | 22.2 | 31.3 | 1.00 |
| `awkrs -j1` | 59.3 ± 11.3 | 51.9 | 99.9 | 2.45 ± 0.49 |
| `awkrs (parallel)` | 142.1 ± 14.3 | 125.8 | 186.4 | 5.88 ± 0.69 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 20.0 ± 5.0 | 10.4 | 31.3 | 1.00 |
| `gawk` | 26.0 ± 6.1 | 17.4 | 55.1 | 1.30 ± 0.45 |
| `awkrs` | 25.1 ± 6.2 | 13.1 | 46.2 | 1.26 ± 0.44 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 40.1 ± 4.6 | 29.8 | 54.7 | 2.53 ± 0.98 |
| `gawk` | 15.8 ± 5.8 | 8.6 | 29.6 | 1.00 |
| `awkrs -j1` | 35.9 ± 5.5 | 29.6 | 59.3 | 2.27 ± 0.90 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.

