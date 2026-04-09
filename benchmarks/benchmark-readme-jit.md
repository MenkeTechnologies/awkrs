# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:22:14
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.9 ± 1.0 | 5.4 | 9.7 | 1.03 ± 0.20 |
| `awkrs (bytecode only)` | 6.7 ± 0.9 | 5.3 | 10.3 | 1.00 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.4 ± 0.9 | 11.2 | 16.1 | 1.66 ± 0.21 |
| `awkrs (bytecode only)` | 8.1 ± 0.9 | 6.6 | 12.0 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.4 ± 1.1 | 12.0 | 17.3 | 1.00 |
| `awkrs (bytecode only)` | 14.4 ± 1.0 | 12.7 | 17.8 | 1.00 ± 0.10 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 18.7 ± 0.8 | 16.9 | 20.9 | 1.00 |
| `awkrs (bytecode only)` | 18.9 ± 1.0 | 17.0 | 22.6 | 1.01 ± 0.07 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.4 ± 1.9 | 4.9 | 30.7 | 1.13 ± 0.33 |
| `awkrs (bytecode only)` | 6.5 ± 0.9 | 5.1 | 9.2 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 66.7 ± 3.4 | 62.4 | 73.5 | 1.35 ± 0.11 |
| `awkrs (bytecode only)` | 49.6 ± 2.9 | 43.8 | 55.8 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 12.3 ± 0.7 | 10.9 | 14.5 | 1.00 |
| `awkrs (bytecode only)` | 13.2 ± 1.0 | 11.0 | 16.2 | 1.07 ± 0.10 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 16.6 ± 1.1 | 14.3 | 19.0 | 1.03 ± 0.10 |
| `awkrs (bytecode only)` | 16.1 ± 1.1 | 13.9 | 19.0 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.2 ± 1.2 | 15.3 | 20.6 | 1.00 |
| `awkrs (bytecode only)` | 20.7 ± 3.5 | 15.3 | 27.2 | 1.20 ± 0.22 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 11.1 ± 3.9 | 6.4 | 24.3 | 1.59 ± 0.85 |
| `awkrs (bytecode only)` | 7.0 ± 2.8 | 4.0 | 37.5 | 1.00 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

