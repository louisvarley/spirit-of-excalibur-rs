#!/usr/bin/env python3
"""Record golden vectors for the animation-script handlers.

The scene engine (FUN_18cb_066a) runs a word-coded script per widget: the
top nibble of each word selects a handler from the far-pointer table at
DS:0389, the low 12 bits are its argument. This records every handler call
(handlers 0..13; 14/15 are the talk and combat hooks) while the original
plays from one or more snapshots: the argument, the widget bytes before and
after, the return value, the clock and the random numbers drawn.
Writes tests/vectors/script_ops.json.

    python tests/oracle/script_vectors.py SNAPSHOT[:TICKS] ...
"""
from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DS = 0x3364 - 0x1000 + LOAD_SEG
WIDGET = 0x36
HANDLERS = [0x08FF, 0x0944, 0x0B06, 0x0BB1, 0x0BDB, 0x0C2D, 0x0C8A, 0x0DE6,
            0x0E5F, 0x0E5F, 0x15C3, 0x15D3, 0x1201, 0x13C4]
PER_HANDLER = 400   # cap per handler and snapshot, to keep the file small


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def record(snap: str, ticks: int) -> list[dict]:
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    seg18 = 0x18CB - 0x1000 + LOAD_SEG
    calls: list[dict] = []
    stack: list[dict] = []
    counts: dict[int, int] = {}

    def widget():
        return m.u16(DS, 0x9A70), m.u16(DS, 0x9A6E)

    def entry(handler: int):
        def h(uc, addr, size, user):
            ss, sp = m.r("ss"), m.r("sp")
            # Only calls from the interpreter (18cb:0780), not internal ones.
            if (m.u16(ss, sp + 2), m.u16(ss, sp)) != (seg18, 0x0784):
                return
            ws, wo = widget()
            # The widget FUN_15b8_16ca(0x220, 0, +0x2E) finds (followed or
            # signalled by handlers 1 and 5).
            other = None
            target_id = m.u16(ws, wo + 0x2E)
            for i in range(m.read(DS, 0x969D, 1)[0]):
                s, o = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
                es, eo = m.u16(s, o + 2), m.u16(s, o)
                if m.u16(es, eo + 0x0A) == target_id and m.u16(es, eo) & 0x220:
                    other = m.read(es, eo, WIDGET).hex()
                    break
            stack.append({
                "other": other,
                "handler": handler, "arg": m.u16(ss, sp + 4),
                "before": m.read(ws, wo, WIDGET).hex(), "ptr": (ws, wo),
                "clock": m.u16(DS, 0x7E3E) | m.u16(DS, 0x7E40) << 16,
                "word": m.u16(DS, 0x9A76), "rng": [],
                # The widget's script (far pointer table DS:1E2A by +0x0E),
                # which the conditional handler reads ahead in.
                "program": m.read(m.u16(DS, 0x1E2A + 4 * m.read(ws, wo + 0x0E, 1)[0] + 2),
                                  m.u16(DS, 0x1E2A + 4 * m.read(ws, wo + 0x0E, 1)[0]), 0x200).hex(),
                "flags_before": m.read(DS, 0x01F5, 1).hex() + m.read(DS, 0x9A78, 1).hex(),
            })
        return h

    def rng(uc, addr, size, user):
        if stack:
            stack[-1]["rng"].append(m.r("ax"))

    def returned(uc, addr, size, user):
        if not stack:
            return
        rec = stack.pop()
        ws, wo = rec.pop("ptr")
        rec["after"] = m.read(ws, wo, WIDGET).hex()
        rec["ret"] = m.r("ax") & 0xFF
        rec["flags_after"] = m.read(DS, 0x01F5, 1).hex() + m.read(DS, 0x9A78, 1).hex()
        n = counts.get(rec["handler"], 0)
        if n < PER_HANDLER:
            calls.append(rec)
        counts[rec["handler"]] = n + 1

    for i, off in enumerate(HANDLERS):
        if i == 9:
            continue  # same routine as 8
        m.uc.hook_add(UC_HOOK_CODE, entry(i), begin=lin(0x18CB, off), end=lin(0x18CB, off))
    m.uc.hook_add(UC_HOOK_CODE, rng, begin=lin(0x15B8, 0x182E), end=lin(0x15B8, 0x182E))
    m.uc.hook_add(UC_HOOK_CODE, returned, begin=lin(0x18CB, 0x0784), end=lin(0x18CB, 0x0784))
    m.run(ticks)
    print(f"{snap}: {len(calls)} calls, per handler {dict(sorted(counts.items()))}", flush=True)
    return calls


def main(argv: list[str]) -> int:
    calls = []
    for arg in argv:
        name, _, ticks = arg.partition(":")
        calls += record(name, int(ticks or 600))
    path = ROOT / "tests" / "vectors" / "script_ops.json"
    path.write_text(json.dumps({"calls": calls}))
    print(f"{len(calls)} calls; wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
