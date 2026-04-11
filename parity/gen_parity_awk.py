#!/usr/bin/env python3
"""Generate machine parity corpus: one deterministic BEGIN-only program per case.

Output: parity/cases/{id}_bulk.awk for id in [start, end] (default 1000–1999).

Bitwise ops use small helper functions (not gawk and/or/xor/lshift/rshift) so the same
files run under **gawk**, **mawk**, and **BSD awk**. Each file is tiny, printf-driven,
no time / ENVIRON / ARGV. Regenerate after changing templates; run:

  bash parity/run_parity.sh gawk
  bash parity/run_parity.sh mawk
  bash parity/run_parity.sh bsd"""

from __future__ import annotations

import argparse
from pathlib import Path


ROOT = Path(__file__).resolve().parent
CASES = ROOT / "cases"


def params(case_id: int) -> dict[str, int]:
    n = case_id
    return {
        "n": n,
        "a": (n * 7) % 97 + 1,
        "b": (n * 13) % 89 + 2,
        "c": (n * 3) % 83 + 3,
        "d": (n * 11) % 79 + 4,
        "m3": n % 3,
        "m5": n % 5,
        "m7": n % 7,
        "m11": n % 11,
        "m17": n % 17,
    }


# Portable bitwise: pb_* helpers are emitted once per file (see PORTABLE_BIT_FUNCS).
TEMPLATES: list[str] = [
    'printf "%d\\n", pb_xor({n}, {a})',
    'printf "%d\\n", int(({a} + {b}) * {c} / 7)',
    'printf "%d\\n", length(sprintf("p%ddq", {n}))',
    'printf "%d\\n", pb_and(15, {a})',
    'printf "%d\\n", pb_or(8, {m3})',
    'printf "%d\\n", pb_lshift(1, {m3} + 1)',
    'printf "%d\\n", pb_rshift(256, {m3} + 1)',
    'printf "%d\\n", match("x{n}yz", /[0-9]+/)',
    'printf "%d\\n", index("alphabet", "h") + {m3}',
    'printf "%s\\n", substr("0123456789", {m5}, 4)',
    'printf "%d\\n", split("{a}:{b}:{c}", t, ":") + length(t[2])',
    'printf "%s\\n", tolower("X{n}Y")',
    'printf "%s\\n", toupper("ab{m5}c")',
    'printf "%d\\n", int(sqrt({c} * {c} + {a}))',
    'printf "%d\\n", int(log({c} + 1) * 10)',
    'printf "%.0f\\n", sin(0) + cos(0) + {m3}',
    # Parenthesize: bare `... > 0` after printf args is a redirect to file "0".
    'printf "%d\\n", (atan2(1, 1) > 0)',
    'printf "%d\\n", int(exp(log({a} + 1.0)))',
    'printf "%s\\n", sprintf("%02x", {m17} + 10)',
    '{{ _v = {m3}; printf "%d\\n", _v ? 5 : 0 }}',
    '{{ s = 0; for (j = 1; j <= {m7} + 1; j++) s += j; printf "%d\\n", s }}',
    'printf "%d\\n", ({a} < {b}) + ({b} < {c}) * 2',
    '{{ x = "a{n}b"; sub(/[0-9]+/, "Z", x); printf "%s\\n", x }}',
    '{{ x = "n{n}n"; gsub(/n/, "m", x); printf "%s\\n", x }}',
    '{{ a1[1] = {a}; a1[2] = {b}; printf "%d\\n", a1[1] + a1[2] }}',
    '{{ split("", a2); a2["k"] = {c}; printf "%d\\n", a2["k"] }}',
    '{{ _s = ""; for (_i = 0; _i < {m5} + 2; _i++) _s = _s "0"; printf "%d\\n", length(_s) }}',
    '{{ u = ""; printf "%d\\n", (u == "") + 1 }}',
    'printf "%d\\n", ("ab" < "ac") + ("x" == "x")',
]

PORTABLE_BIT_FUNCS = r"""function pb_and(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if (ai && bi) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_or(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if (ai || bi) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_xor(a, b,    r, i, ai, bi, p) {
    r = 0
    p = 1
    for (i = 0; i < 32; i++) {
        ai = a % 2
        a = int(a / 2)
        bi = b % 2
        b = int(b / 2)
        if ((ai && !bi) || (!ai && bi)) {
            r += p
        }
        p *= 2
        if (a == 0 && b == 0) {
            break
        }
    }
    return r
}
function pb_lshift(x, n,    i) {
    for (i = 0; i < n; i++) {
        x *= 2
    }
    return x
}
function pb_rshift(x, n,    i) {
    for (i = 0; i < n; i++) {
        x = int(x / 2)
    }
    return x
}"""


def render(case_id: int) -> str:
    p = params(case_id)
    # Pick a stable template from id so neighboring files differ.
    lines: list[str] = [f"# bulk:{case_id}", PORTABLE_BIT_FUNCS, "BEGIN {"]
    n_templates = len(TEMPLATES)
    for off in range(4):
        idx = (case_id + off * 17) % n_templates
        tpl = TEMPLATES[idx]
        lines.append(f"    {tpl.format(**p)}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--start", type=int, default=1000)
    ap.add_argument("--end", type=int, default=1999)
    args = ap.parse_args()
    CASES.mkdir(parents=True, exist_ok=True)
    for case_id in range(args.start, args.end + 1):
        path = CASES / f"{case_id}_bulk.awk"
        path.write_text(render(case_id), encoding="utf-8")
    n = args.end - args.start + 1
    print(f"wrote {n} machine parity cases to {CASES}/<id>_bulk.awk")


if __name__ == "__main__":
    main()
