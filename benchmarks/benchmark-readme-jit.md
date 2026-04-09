# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:13:02
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.2 ± 2.2 | 3.5 | 18.5 | 1.67 ± 0.63 |
| `awkrs (bytecode only)` | 4.3 ± 1.0 | 2.4 | 9.4 | 1.00 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 15.9 ± 1.3 | 12.6 | 19.1 | 1.76 ± 0.24 |
| `awkrs (bytecode only)` | 9.0 ± 1.0 | 7.0 | 12.9 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.4 ± 1.1 | 12.2 | 18.4 | 1.00 |
| `awkrs (bytecode only)` | 14.7 ± 1.2 | 12.4 | 18.5 | 1.02 ± 0.11 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 25.2 ± 6.2 | 17.7 | 45.6 | 1.00 |
| `awkrs (bytecode only)` | 33.4 ± 7.9 | 25.5 | 70.5 | 1.32 ± 0.45 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 9.5 ± 7.9 | 1.9 | 46.0 | 3.79 ± 3.81 |
| `awkrs (bytecode only)` | 2.5 ± 1.4 | 0.0 | 12.1 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 90.1 ± 10.7 | 77.2 | 109.2 | 1.43 ± 0.20 |
| `awkrs (bytecode only)` | 62.9 ± 4.7 | 56.4 | 75.6 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.3 ± 1.2 | 10.8 | 16.5 | 1.06 ± 0.12 |
| `awkrs (bytecode only)` | 12.6 ± 0.8 | 11.3 | 16.2 | 1.00 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.1 ± 1.0 | 15.1 | 20.1 | 1.01 ± 0.10 |
| `awkrs (bytecode only)` | 17.0 ± 1.5 | 14.4 | 20.9 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 19.0 ± 1.7 | 16.1 | 23.7 | 1.00 |
| `awkrs (bytecode only)` | 19.9 ± 1.5 | 16.1 | 23.1 | 1.05 ± 0.12 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.8 ± 1.1 | 5.4 | 10.3 | 1.15 ± 0.22 |
| `awkrs (bytecode only)` | 6.8 ± 0.9 | 5.1 | 9.4 | 1.00 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

