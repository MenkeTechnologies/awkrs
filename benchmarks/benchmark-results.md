# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-09 16:58:13
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 42.1 ± 8.3 | 34.8 | 69.4 | 14.21 ± 4.77 |
| `gawk` | 23.2 ± 1.6 | 20.5 | 28.8 | 7.82 ± 2.19 |
| `awkrs` | 3.0 ± 0.8 | 1.7 | 5.5 | 1.00 |
| `awkrs (parallel)` | 294.3 ± 6.0 | 285.1 | 306.6 | 99.41 ± 27.03 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 18.4 ± 4.9 | 14.1 | 49.4 | 1.46 ± 0.41 |
| `gawk` | 26.5 ± 7.4 | 17.9 | 60.3 | 2.11 ± 0.61 |
| `awkrs` | 12.6 ± 1.0 | 10.5 | 16.3 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 33.4 ± 2.3 | 29.7 | 39.4 | 1.60 ± 0.28 |
| `gawk` | 20.9 ± 3.4 | 17.6 | 46.4 | 1.00 |
| `awkrs` | 26.5 ± 3.1 | 19.6 | 33.9 | 1.27 ± 0.25 |

## 4. awkrs: JIT vs bytecode VM

Same **awkrs** binary: default path (JIT attempted for eligible chunks) vs `AWKRS_JIT=0` (bytecode interpreter only). Use `env -u AWKRS_JIT` so a shell `AWKRS_JIT` alias does not skew the “JIT on” run.

### 4a. CPU-bound BEGIN (same program as §2)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.8 ± 1.7 | 10.6 | 18.3 | 1.32 ± 1.56 |
| `awkrs (bytecode only)` | 10.5 ± 12.3 | 2.2 | 96.0 | 1.00 |

### 4b. Sum first column (same program and input as §3)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 15.9 ± 1.0 | 13.2 | 18.3 | 1.00 |
| `awkrs (bytecode only)` | 16.8 ± 19.6 | 12.9 | 255.9 | 1.06 ± 1.23 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. **§4** compares JIT vs bytecode for the same awkrs workloads. Re-run after `cargo build --release` on your hardware.

