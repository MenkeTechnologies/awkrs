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
| `BSD awk` | 78.1 ± 9.2 | 68.8 | 110.2 | 4.07 ± 0.50 |
| `gawk` | 26.1 ± 1.3 | 24.5 | 29.0 | 1.36 ± 0.08 |
| `awkrs -j1` | 19.2 ± 0.7 | 18.0 | 20.4 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.2 ± 1.0 | 12.2 | 15.6 | 1.00 |
| `gawk` | 18.6 ± 0.9 | 17.1 | 21.3 | 1.31 ± 0.11 |
| `awkrs` | 15.6 ± 0.4 | 14.9 | 16.2 | 1.10 ± 0.08 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 72.5 ± 8.6 | 62.1 | 94.9 | 5.32 ± 0.65 |
| `gawk` | 18.0 ± 0.7 | 16.9 | 19.9 | 1.32 ± 0.06 |
| `awkrs -j1` | 13.6 ± 0.4 | 12.9 | 14.6 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
