#!/usr/bin/env python3
"""Record golden vectors for FUN_20b5_1705 (what follows a unit leaving the
fight: a new opponent, or the end of the battle) and FUN_20b5_1229 (a dead
character's bookkeeping), which it calls.

Runs the original from a battle snapshot. For every 1705 call it records
DGROUP and the whole widget list before and after, the widget argument (as
a list index), the flag, the uninitialised [BP-1] byte, the random numbers
drawn, and the calls it makes to routines that are not ported with it
(music, stat changes, map cooldowns, UI, story events), with arguments.
Writes tests/vectors/rout_calls.json.

    python tests/oracle/rout_vectors.py [SNAPSHOT[:TICKS] ...]
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
# Far calls made from 1705/1229: (seg, off) -> (name, number of word args).
SUBCALLS = {
    (0x1316, 0x187B): ("music", 4), (0x1BA8, 0x2611): ("stat_change", 3),
    (0x1B06, 0x071C): ("map_071c", 2), (0x1B06, 0x06B5): ("map_06b5", 3),
    (0x1772, 0x0623): ("ui_0623", 0), (0x1772, 0x072B): ("ui_072b", 0),
    (0x15B8, 0x1876): ("ui_1876", 1), (0x26C5, 0x000B): ("story_event", 2),
}


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def record(snap: str, ticks: int) -> list[dict]:
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    calls: list[dict] = []
    cur: dict | None = None

    def ptrs():
        out = []
        for i in range(m.read(DS, 0x969D, 1)[0]):
            s, o = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
            out.append((m.u16(s, o + 2), m.u16(s, o)))
        return out

    def state():
        return {"ds": m.read(DS, 0, 0x10000).hex(),
                "widgets": [m.read(s, o, WIDGET).hex() for s, o in ptrs()],
                "current": m.u16(DS, 0x9A6C)}

    def entry(uc, addr, size, user):
        nonlocal cur
        ss, sp = m.r("ss"), m.r("sp")
        # Far call: [SP] IP, [SP+2] CS, then the widget (off, seg) and flag.
        wo, ws, flag = m.u16(ss, sp + 4), m.u16(ss, sp + 6), m.read(ss, sp + 8, 1)[0]
        index = ptrs().index((ws, wo)) if (ws, wo) in ptrs() else None
        # [BP-1] once PUSH BP has run (BP = SP-2).
        stale = m.read(ss, (sp - 3) & 0xFFFF, 1)[0]
        # SS:SP as a DGROUP offset: the stack lies inside DGROUP's 64K, so
        # comparisons must skip the region below it.
        cur = {"widget": index, "flag": flag, "stale_bp1": stale, "rng": [], "subcalls": [],
               "sp": ((ss - DS) << 4) + sp, "before": state()}

    def leave(uc, addr, size, user):
        nonlocal cur
        if cur is None:
            return
        cur["ret"] = m.r("ax") & 0xFF
        cur["after"] = state()
        calls.append(cur)
        cur = None

    def rng(uc, addr, size, user):
        if cur is not None:
            cur["rng"].append(m.r("ax"))

    def sub(name, nargs):
        def h(uc, addr, size, user):
            if cur is None:
                return
            ss, sp = m.r("ss"), m.r("sp")
            cur["subcalls"].append([name] + [m.u16(ss, sp + 4 + 2 * k) for k in range(nargs)])
        return h

    hook = lambda f, seg, off: m.uc.hook_add(UC_HOOK_CODE, f, begin=lin(seg, off), end=lin(seg, off))
    hook(entry, 0x20B5, 0x1705)
    hook(leave, 0x20B5, 0x1DFE)
    hook(rng, 0x15B8, 0x182E)
    for (seg, off), (name, nargs) in SUBCALLS.items():
        hook(sub(name, nargs), seg, off)
    for _ in range(ticks // 100):
        m.run(100)
    print(f"{snap}: {len(calls)} calls, flags {[c['flag'] for c in calls]}", flush=True)
    return calls


def main(argv: list[str]) -> int:
    calls = []
    for arg in argv or ["duel2:2500"]:
        name, _, ticks = arg.partition(":")
        calls += record(name, int(ticks or 2500))
    path = ROOT / "tests" / "vectors" / "rout_calls.json"
    path.write_text(json.dumps({"calls": calls}))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
