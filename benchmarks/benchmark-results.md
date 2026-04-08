# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-08 16:54:35
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **mawk:** `/opt/homebrew/bin/mawk` (`mawk 1.3.4`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.0`)

Measured with `hyperfine --shell=none` (no shell overhead in measurement).

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 77.9 ± 5.2 | 70.9 | 89.9 | 15.07 ± 1.29 |
| `gawk` | 26.6 ± 1.3 | 24.9 | 31.8 | 5.14 ± 0.37 |
| `mawk` | 18.7 ± 1.3 | 17.1 | 22.8 | 3.62 ± 0.32 |
| `awkrs -j1` | 5.2 ± 0.3 | 4.8 | 6.1 | 1.00 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.4 ± 0.8 | 13.9 | 17.1 | 2.93 ± 0.19 |
| `gawk` | 20.1 ± 0.9 | 18.5 | 22.0 | 3.81 ± 0.23 |
| `mawk` | 9.3 ± 0.5 | 8.2 | 10.4 | 1.77 ± 0.12 |
| `awkrs` | 5.3 ± 0.2 | 4.8 | 5.9 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is shown with `-j1` only.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 68.4 ± 4.8 | 64.0 | 87.4 | 6.74 ± 0.57 |
| `gawk` | 18.4 ± 0.8 | 17.4 | 20.4 | 1.82 ± 0.12 |
| `mawk` | 12.3 ± 0.5 | 11.3 | 13.6 | 1.21 ± 0.08 |
| `awkrs -j1` | 10.1 ± 0.5 | 9.2 | 11.0 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. Re-run after `cargo build --release` on your hardware.
