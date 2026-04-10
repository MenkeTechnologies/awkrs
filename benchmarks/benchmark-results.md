# awkrs vs awk benchmarks

This file is **generated** by `./scripts/benchmark-vs-awk.sh`. Do not edit by hand.

Each `§N` section below is a **single** `hyperfine` invocation with every available engine (BSD awk, gawk, mawk, awkrs JIT-default, awkrs `AWKRS_JIT=0` bytecode-only) on the same input, so the *Relative* column is apples-to-apples within each table. Input size: **1 M** lines (override with `AWKRS_BENCH_LINES=500000 ./scripts/benchmark-vs-awk.sh`). Sizes smaller than ~500 K will put `{ print $1 }`-style workloads below hyperfine's shell-startup noise floor (< 5 ms), so the mean becomes unreliable even with more runs. Workloads mirror [README.md](../README.md) **[0x06] BENCHMARKS** §1–§10. For the focused awkrs-only JIT vs bytecode A/B (same programs), see [`benchmark-readme-jit.md`](benchmark-readme-jit.md) from `./scripts/benchmark-readme-jit-vs-vm.sh`.

## Environment

- **Generated at (UTC):** 2026-04-10 08:41:27
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awk:** `/usr/bin/awk`
- **gawk:** `/opt/homebrew/bin/gawk` (`GNU Awk 5.4.0, API 4.1, PMA Avon 8-g1, (GNU MPFR 4.2.2, GNU MP 6.3.0)`)
- **mawk:** `/opt/homebrew/bin/mawk` (`mawk 1.3.4 20260302`)
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.18`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `env AWKRS_JIT=0 …` (same binary).

## 1. Throughput: `{ print $1 }` (1 M × 1 field)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 195.0 ± 12.1 | 179.8 | 221.6 | 12.43 ± 1.19 |
| `gawk` | 100.8 ± 5.7 | 92.8 | 115.8 | 6.42 ± 0.59 |
| `mawk` | 66.2 ± 3.6 | 61.9 | 78.4 | 4.22 ± 0.38 |
| `awkrs (JIT)` | 15.7 ± 1.1 | 13.3 | 19.6 | 1.00 |
| `awkrs (bytecode)` | 16.1 ± 1.3 | 13.1 | 20.2 | 1.03 ± 0.11 |

## 2. CPU-bound BEGIN (no input, 400 K-iter loop)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 15.8 ± 0.9 | 14.0 | 18.6 | 1.71 ± 0.14 |
| `gawk` | 20.7 ± 0.8 | 18.8 | 22.9 | 2.24 ± 0.16 |
| `mawk` | 9.7 ± 0.6 | 8.3 | 11.4 | 1.06 ± 0.09 |
| `awkrs (JIT)` | 9.2 ± 0.5 | 8.4 | 12.0 | 1.00 |
| `awkrs (bytecode)` | 9.6 ± 0.7 | 8.2 | 12.0 | 1.04 ± 0.09 |

## 3. Sum first column: `{ s += $1 } END { print s }` (1 M × 1 field)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 158.5 ± 7.8 | 147.0 | 172.7 | 12.27 ± 1.02 |
| `gawk` | 62.9 ± 2.3 | 58.4 | 68.9 | 4.87 ± 0.37 |
| `mawk` | 37.5 ± 1.2 | 33.7 | 39.9 | 2.90 ± 0.22 |
| `awkrs (JIT)` | 13.0 ± 0.7 | 11.9 | 15.4 | 1.01 ± 0.09 |
| `awkrs (bytecode)` | 12.9 ± 0.9 | 11.5 | 16.1 | 1.00 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (1 M × 5 fields)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 647.6 ± 20.7 | 623.5 | 686.3 | 11.60 ± 0.54 |
| `gawk` | 266.1 ± 12.5 | 257.4 | 301.8 | 4.77 ± 0.28 |
| `mawk` | 156.6 ± 5.8 | 149.8 | 170.7 | 2.81 ± 0.14 |
| `awkrs (JIT)` | 56.4 ± 2.0 | 53.1 | 61.8 | 1.01 ± 0.05 |
| `awkrs (bytecode)` | 55.8 ± 1.9 | 53.4 | 61.6 | 1.00 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (1 M, no matches)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 191.8 ± 9.7 | 180.1 | 208.9 | 17.31 ± 1.29 |
| `gawk` | 351.4 ± 6.4 | 342.7 | 363.3 | 31.72 ± 1.84 |
| `mawk` | 19.3 ± 0.9 | 17.5 | 21.8 | 1.74 ± 0.12 |
| `awkrs (JIT)` | 11.1 ± 0.6 | 9.5 | 13.5 | 1.00 |
| `awkrs (bytecode)` | 11.1 ± 0.7 | 9.5 | 14.6 | 1.00 ± 0.08 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }` (1 M × 5 fields)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 826.2 ± 31.8 | 792.2 | 896.0 | 2.43 ± 0.16 |
| `gawk` | 342.4 ± 10.2 | 330.6 | 362.5 | 1.01 ± 0.06 |
| `mawk` | 610.0 ± 17.2 | 588.9 | 648.7 | 1.79 ± 0.11 |
| `awkrs (JIT)` | 340.0 ± 19.0 | 324.2 | 377.7 | 1.00 |
| `awkrs (bytecode)` | 343.7 ± 12.0 | 323.5 | 356.7 | 1.01 ± 0.07 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }` (1 M × 2 fields)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 289.1 ± 19.8 | 263.1 | 321.1 | 9.58 ± 0.78 |
| `gawk` | 116.1 ± 3.7 | 111.0 | 124.4 | 3.85 ± 0.21 |
| `mawk` | 71.1 ± 3.3 | 66.9 | 83.6 | 2.36 ± 0.15 |
| `awkrs (JIT)` | 30.2 ± 1.3 | 28.1 | 34.0 | 1.00 |
| `awkrs (bytecode)` | 30.7 ± 1.3 | 28.0 | 35.5 | 1.02 ± 0.06 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }` (1 M × 2 fields)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 261.8 ± 10.5 | 251.4 | 280.8 | 13.96 ± 0.89 |
| `gawk` | 100.5 ± 3.3 | 95.3 | 109.5 | 5.36 ± 0.32 |
| `mawk` | 57.7 ± 1.5 | 54.5 | 61.1 | 3.08 ± 0.17 |
| `awkrs (JIT)` | 19.0 ± 1.0 | 17.6 | 23.0 | 1.01 ± 0.07 |
| `awkrs (bytecode)` | 18.8 ± 0.9 | 17.5 | 22.8 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }` (1 M × 5 fields)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 640.8 ± 26.6 | 611.9 | 689.3 | 12.68 ± 0.62 |
| `gawk` | 182.2 ± 7.7 | 168.1 | 197.2 | 3.61 ± 0.18 |
| `mawk` | 121.0 ± 3.8 | 113.6 | 128.1 | 2.39 ± 0.10 |
| `awkrs (JIT)` | 51.0 ± 1.3 | 49.2 | 53.8 | 1.01 ± 0.04 |
| `awkrs (bytecode)` | 50.5 ± 1.3 | 48.8 | 54.8 | 1.00 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (1 M × 1 field, no matches)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `BSD awk` | 291.5 ± 7.0 | 282.3 | 300.4 | 21.15 ± 0.98 |
| `gawk` | 436.3 ± 11.9 | 425.7 | 459.3 | 31.66 ± 1.53 |
| `mawk` | 74.3 ± 3.7 | 68.8 | 84.2 | 5.39 ± 0.34 |
| `awkrs (JIT)` | 13.8 ± 0.5 | 12.8 | 16.2 | 1.00 |
| `awkrs (bytecode)` | 13.9 ± 0.9 | 12.7 | 17.6 | 1.01 ± 0.08 |

---

Re-run after `cargo build --release` on your hardware. Install mawk (`brew install mawk` / `apt install mawk`) and gawk for full cross-engine tables; without them the table simply omits that row. §6 iteration order differs across engines, so its output is not compared — only the mean time.

