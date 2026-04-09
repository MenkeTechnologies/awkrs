# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 20:17:48
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.2 ± 0.6 | 5.2 | 8.1 | 1.00 |
| `awkrs (bytecode only)` | 6.2 ± 0.7 | 5.2 | 8.5 | 1.00 ± 0.15 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.3 ± 1.0 | 11.4 | 15.8 | 1.69 ± 0.18 |
| `awkrs (bytecode only)` | 7.8 ± 0.6 | 6.7 | 9.8 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.6 ± 0.6 | 4.8 | 7.2 | 1.00 |
| `awkrs (bytecode only)` | 5.7 ± 0.7 | 4.5 | 9.0 | 1.01 ± 0.16 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.5 ± 0.6 | 13.3 | 16.1 | 1.00 |
| `awkrs (bytecode only)` | 15.1 ± 1.0 | 13.3 | 18.1 | 1.04 ± 0.08 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.8 ± 0.7 | 4.6 | 8.4 | 1.03 ± 0.14 |
| `awkrs (bytecode only)` | 5.7 ± 0.4 | 4.9 | 7.1 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 62.0 ± 2.7 | 57.6 | 69.9 | 1.41 ± 0.10 |
| `awkrs (bytecode only)` | 43.9 ± 2.6 | 40.1 | 53.5 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 9.0 ± 0.7 | 7.8 | 11.7 | 1.00 ± 0.11 |
| `awkrs (bytecode only)` | 8.9 ± 0.7 | 7.7 | 10.8 | 1.00 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.1 ± 0.6 | 6.2 | 9.0 | 1.00 |
| `awkrs (bytecode only)` | 7.2 ± 0.7 | 6.0 | 9.2 | 1.02 ± 0.13 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.7 ± 0.8 | 12.4 | 15.9 | 1.00 |
| `awkrs (bytecode only)` | 13.9 ± 0.8 | 12.4 | 16.4 | 1.01 ± 0.08 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.2 ± 0.6 | 5.1 | 7.8 | 1.01 ± 0.14 |
| `awkrs (bytecode only)` | 6.1 ± 0.6 | 5.2 | 8.2 | 1.00 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

