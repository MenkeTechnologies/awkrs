# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

## Environment

- **Generated at (UTC):** 2026-04-09 17:01:50
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

## 1. Throughput: print first field

Input: **200000** lines from `seq 1 200000` (one field per line). Program: `{ print $1 }`.

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 39.7 ± 2.4 | 36.5 | 50.5 | 8.24 ± 1.35 |
| `gawk` | 24.4 ± 1.5 | 21.1 | 29.2 | 5.07 ± 0.83 |
| `awkrs` | 4.8 ± 0.7 | 3.6 | 7.6 | 1.00 |
| `awkrs (parallel)` | 294.6 ± 8.9 | 284.3 | 313.7 | 61.14 ± 9.52 |

## 2. CPU-bound BEGIN (no input)

Program: `BEGIN { s = 0; for (i = 1; i < 400001; i = i + 1) s += i; print s }` (stdin empty; `<` avoids a parser limitation on `<=` in this `for`).

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.4 ± 1.0 | 13.2 | 18.3 | 1.23 ± 0.12 |
| `gawk` | 19.4 ± 1.1 | 17.1 | 21.6 | 1.55 ± 0.14 |
| `awkrs` | 12.5 ± 0.9 | 10.5 | 15.2 | 1.00 |

## 3. Sum first column (single-threaded)

Same input as §1. Program: `{ s += $1 } END { print s }`. (Cross-record state is not parallel-safe in awkrs, so **awkrs** is single-threaded by default here.)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 31.9 ± 1.7 | 29.0 | 38.7 | 2.46 ± 0.24 |
| `gawk` | 16.5 ± 1.0 | 14.4 | 18.7 | 1.27 ± 0.13 |
| `awkrs` | 13.0 ± 1.1 | 10.7 | 17.0 | 1.00 |

## 4. awkrs: JIT vs bytecode VM

Same **awkrs** binary: default path (JIT attempted for eligible chunks) vs `AWKRS_JIT=0` (bytecode interpreter only). Use `env -u AWKRS_JIT` so a shell `AWKRS_JIT` alias does not skew the “JIT on” run.

### 4a. CPU-bound BEGIN (same program as §2)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.3 ± 1.3 | 11.1 | 16.8 | 1.61 ± 0.23 |
| `awkrs (bytecode only)` | 8.3 ± 0.8 | 6.7 | 11.5 | 1.00 |

### 4b. Sum first column (same program and input as §3)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.1 ± 1.1 | 11.7 | 17.3 | 1.05 ± 0.12 |
| `awkrs (bytecode only)` | 13.3 ± 1.1 | 11.5 | 16.5 | 1.00 |

### 4c. Throughput: print first field (same program and input as §1)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.4 ± 0.9 | 5.4 | 9.5 | 1.00 |
| `awkrs (bytecode only)` | 7.9 ± 2.9 | 5.8 | 47.0 | 1.07 ± 0.41 |

### 4d. Parallel `-j8` (same program and input as §1 `awkrs (parallel)`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 329.4 ± 13.6 | 305.3 | 346.9 | 2.70 ± 0.14 |
| `awkrs (bytecode only)` | 122.2 ± 4.1 | 113.1 | 128.0 | 1.00 |

---

Throughput (§1) can use **awkrs `-j`** when the program is parallel-safe; **BEGIN-only** (§2) and **accumulators** (§3) are effectively single-threaded here. **§4** compares JIT vs bytecode for every **awkrs** workload in §1–§3 (4a = §2, 4b = §3, 4c–4d = §1). Re-run after `cargo build --release` on your hardware.

