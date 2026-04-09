# awkrs README: JIT vs bytecode (§1–§10)

This file is **generated** by `./scripts/benchmark-readme-jit-vs-vm.sh`. Do not edit by hand.

Workloads match [README.md](../README.md) **[0x06] BENCHMARKS** sections **1–10** (same awk programs; **200000** input lines per README except §2, which is BEGIN-only on empty stdin).

## Environment

- **Generated at (UTC):** 2026-04-09 17:30:58
- **uname:** `Darwin 25.4.0 arm64`
- **CPU (macOS sysctl):** Apple M5 Max
- **awkrs:** `/Users/wizard/RustroverProjects/awkrs/target/release/awkrs` (`awkrs 0.1.13`)

**JIT on:** `env -u AWKRS_JIT …` — **JIT off:** `AWKRS_JIT=0 …`

## 1. Throughput: `{ print $1 }` (one field per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 7.2 ± 1.2 | 4.4 | 10.5 | 1.00 |
| `awkrs (bytecode only)` | 7.4 ± 1.8 | 4.5 | 15.5 | 1.03 ± 0.30 |

## 2. CPU-bound BEGIN (no input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.3 ± 1.2 | 11.8 | 17.7 | 1.68 ± 0.25 |
| `awkrs (bytecode only)` | 8.5 ± 1.1 | 6.7 | 12.5 | 1.00 |

## 3. Sum first column: `{ s += $1 } END { print s }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 14.1 ± 2.5 | 10.2 | 34.7 | 1.11 ± 0.23 |
| `awkrs (bytecode only)` | 12.8 ± 1.3 | 10.1 | 17.6 | 1.00 |

## 4. Multi-field print: `{ print $1, $3, $5 }` (five fields per line)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 17.9 ± 0.8 | 16.6 | 20.9 | 1.00 |
| `awkrs (bytecode only)` | 17.9 ± 0.8 | 16.1 | 20.2 | 1.00 ± 0.06 |

## 5. Regex filter: `/alpha/ { c += 1 } END { print c }` (lines have no `alpha`)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.1 ± 0.8 | 4.8 | 8.9 | 1.01 ± 0.22 |
| `awkrs (bytecode only)` | 6.0 ± 1.0 | 4.6 | 9.4 | 1.00 |

## 6. Associative array: `{ a[$5] += 1 } END { for (k in a) print k, a[k] }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 63.4 ± 2.7 | 59.5 | 70.3 | 1.26 ± 0.14 |
| `awkrs (bytecode only)` | 50.2 ± 5.2 | 42.3 | 64.4 | 1.00 |

## 7. Conditional field: `NR % 2 == 0 { print $2 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 12.9 ± 1.3 | 10.6 | 18.6 | 1.00 |
| `awkrs (bytecode only)` | 14.0 ± 1.8 | 11.0 | 18.6 | 1.09 ± 0.18 |

## 8. Field computation: `{ sum += $1 * $2 } END { print sum }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 15.7 ± 1.3 | 13.4 | 18.7 | 1.00 |
| `awkrs (bytecode only)` | 15.7 ± 1.6 | 13.0 | 21.0 | 1.00 ± 0.13 |

## 9. String concat print: `{ print $3 "-" $5 }`

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 16.6 ± 1.3 | 14.5 | 20.3 | 1.03 ± 0.11 |
| `awkrs (bytecode only)` | 16.1 ± 1.1 | 14.2 | 19.1 | 1.00 |

## 10. gsub: `{ gsub("alpha", "ALPHA"); print }` (no `alpha` in input)

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `awkrs (JIT default)` | 6.9 ± 1.1 | 5.0 | 10.1 | 1.00 |
| `awkrs (bytecode only)` | 6.9 ± 0.7 | 5.4 | 9.0 | 1.01 ± 0.19 |


---

Re-run after `cargo build --release` on your hardware. For cross-engine tables (BSD awk / gawk / mawk / awkrs), see [`benchmark-results.md`](benchmark-results.md) from `./scripts/benchmark-vs-awk.sh`.

