# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-09 19:37:20
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.14`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 38.9 ± 3.4 | 35.8 | 50.4 | 8.44 ± 1.43 |
| `gawk` | 24.2 ± 1.4 | 22.2 | 29.4 | 5.25 ± 0.83 |
| `awkrs` | 4.6 ± 0.7 | 3.4 | 6.9 | 1.00 |
| `awkrs (parallel)` | 269.4 ± 18.6 | 254.7 | 308.5 | 58.38 ± 9.41 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 14.9 ± 0.9 | 12.7 | 17.2 | 1.17 ± 0.12 |
| `gawk` | 20.1 ± 2.1 | 17.6 | 37.1 | 1.58 ± 0.21 |
| `awkrs` | 12.7 ± 1.0 | 11.1 | 15.8 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 34.4 ± 5.0 | 28.7 | 68.0 | 6.80 ± 1.40 |
| `gawk` | 16.9 ± 0.8 | 15.2 | 20.6 | 3.34 ± 0.51 |
| `awkrs` | 5.1 ± 0.7 | 4.0 | 10.4 | 1.00 |

## 4. awkrs: JIT vs bytecode VM

Same **awkrs** binary: default path (JIT attempted for eligible chunks) vs `AWKRS_JIT=0` (bytecode interpreter only). Use `env -u AWKRS_JIT` so a shell `AWKRS_JIT` alias does not skew the “JIT on” run.

### 4a. CPU-bound BEGIN (same program as §2)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.4 ± 1.0 | 11.9 | 16.7 | 1.68 ± 0.21 |
| `awkrs (bytecode only)` | 8.0 ± 0.8 | 6.7 | 12.5 | 1.00 |

### 4b. Sum first column (same program and input as §3)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.3 ± 0.5 | 4.4 | 8.0 | 1.00 |
| `awkrs (bytecode only)` | 5.8 ± 0.7 | 4.3 | 10.1 | 1.09 ± 0.18 |

### 4c. Throughput: print first field (same program and input as §1)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.3 ± 0.7 | 4.9 | 8.8 | 1.01 ± 0.18 |
| `awkrs (bytecode only)` | 6.2 ± 0.8 | 4.6 | 9.6 | 1.00 |

### 4d. Parallel `-j8` (same program and input as §1 `awkrs (parallel)`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 282.9 ± 22.0 | 254.8 | 308.2 | 2.62 ± 0.22 |
| `awkrs (bytecode only)` | 107.8 ± 3.1 | 103.2 | 116.1 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. **§4** compares JIT vs bytecode for every **awkrs** workload in §1–§3 (4a = §2, 4b = §3, 4c–4d = §1). Re-run after `cargo build --release` on your hardware.

