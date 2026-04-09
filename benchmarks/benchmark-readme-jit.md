# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:44:11
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.7 ± 1.2 | 5.3 | 12.4 | 1.00 |
| `awkrs (bytecode only)` | 8.4 ± 1.3 | 5.4 | 11.2 | 1.09 ± 0.24 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 16.5 ± 1.0 | 14.5 | 19.3 | 1.73 ± 0.17 |
| `awkrs (bytecode only)` | 9.5 ± 0.7 | 8.1 | 13.7 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 13.6 ± 1.1 | 10.5 | 16.0 | 1.05 ± 0.13 |
| `awkrs (bytecode only)` | 12.9 ± 1.1 | 10.7 | 15.9 | 1.00 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 18.3 ± 1.0 | 16.0 | 21.3 | 1.06 ± 0.07 |
| `awkrs (bytecode only)` | 17.3 ± 0.8 | 15.7 | 19.7 | 1.00 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 5.7 ± 0.8 | 4.4 | 8.9 | 1.00 |
| `awkrs (bytecode only)` | 6.3 ± 0.9 | 4.2 | 8.8 | 1.10 ± 0.22 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 70.0 ± 4.9 | 61.6 | 81.7 | 1.42 ± 0.13 |
| `awkrs (bytecode only)` | 49.1 ± 3.0 | 44.7 | 59.2 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 12.5 ± 0.7 | 11.1 | 15.0 | 1.00 |
| `awkrs (bytecode only)` | 12.9 ± 1.0 | 11.2 | 15.0 | 1.03 ± 0.10 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 16.1 ± 1.4 | 13.8 | 19.7 | 1.03 ± 0.11 |
| `awkrs (bytecode only)` | 15.7 ± 0.9 | 13.6 | 17.9 | 1.00 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.3 ± 1.4 | 15.0 | 22.7 | 1.00 |
| `awkrs (bytecode only)` | 17.3 ± 1.3 | 14.8 | 21.0 | 1.00 ± 0.11 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.6 ± 0.9 | 4.9 | 9.8 | 1.00 |
| `awkrs (bytecode only)` | 7.5 ± 1.3 | 5.2 | 15.2 | 1.14 ± 0.26 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

