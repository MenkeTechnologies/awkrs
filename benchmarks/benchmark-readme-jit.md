# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:27:11
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.6 ± 2.1 | 5.6 | 27.3 | 1.00 |
| `awkrs (bytecode only)` | 8.6 ± 1.7 | 5.9 | 17.0 | 1.13 ± 0.39 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.3 ± 1.0 | 12.0 | 16.7 | 1.72 ± 0.23 |
| `awkrs (bytecode only)` | 8.3 ± 0.9 | 6.8 | 13.1 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 15.4 ± 1.8 | 12.3 | 19.5 | 1.15 ± 0.15 |
| `awkrs (bytecode only)` | 13.4 ± 0.8 | 11.9 | 15.9 | 1.00 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 18.3 ± 0.9 | 16.7 | 20.8 | 1.00 |
| `awkrs (bytecode only)` | 18.5 ± 1.1 | 16.4 | 23.0 | 1.01 ± 0.08 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.0 ± 0.9 | 4.5 | 8.5 | 1.03 ± 0.24 |
| `awkrs (bytecode only)` | 5.9 ± 1.0 | 4.3 | 9.6 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 64.8 ± 3.4 | 59.5 | 74.4 | 1.35 ± 0.11 |
| `awkrs (bytecode only)` | 48.2 ± 3.2 | 43.9 | 56.0 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.3 ± 1.5 | 11.3 | 20.0 | 1.00 |
| `awkrs (bytecode only)` | 15.2 ± 1.6 | 11.4 | 19.1 | 1.14 ± 0.17 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 24.0 ± 4.9 | 16.7 | 42.2 | 1.36 ± 0.30 |
| `awkrs (bytecode only)` | 17.7 ± 1.3 | 15.8 | 24.0 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.8 ± 1.0 | 15.8 | 21.7 | 1.00 |
| `awkrs (bytecode only)` | 18.0 ± 1.1 | 15.8 | 20.4 | 1.01 ± 0.08 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.8 ± 1.0 | 4.9 | 11.0 | 1.00 |
| `awkrs (bytecode only)` | 7.3 ± 0.8 | 5.4 | 9.6 | 1.07 ± 0.20 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

