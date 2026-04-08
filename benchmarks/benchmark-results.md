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
| `BSD awk` | 79.5 ± 7.5 | 70.1 | 93.8 | 5.77 ± 0.62 |
| `gawk` | 26.4 ± 2.3 | 23.8 | 31.8 | 1.91 ± 0.19 |
| `awkrs -j1` | 13.8 ± 0.7 | 13.0 | 15.9 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.8 ± 1.1 | 13.2 | 16.8 | 1.36 ± 0.14 |
| `gawk` | 19.6 ± 1.4 | 17.5 | 22.4 | 1.81 ± 0.18 |
| `awkrs` | 10.9 ± 0.8 | 9.8 | 12.3 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 68.4 ± 6.0 | 61.0 | 80.1 | 5.06 ± 0.52 |
| `gawk` | 16.9 ± 0.6 | 15.5 | 18.0 | 1.25 ± 0.08 |
| `awkrs -j1` | 13.5 ± 0.7 | 12.7 | 16.0 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
