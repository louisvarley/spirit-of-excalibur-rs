#!/usr/bin/env python3
"""Record golden vectors for FUN_2920_0a12 (map encounter check).

Hooks every real call of the collision callback while the original plays from
the given snapshots and records its inputs (character records, location table,
entity views, random roll, combat result) and outputs (return value, final
subject/object, buttons, dialog height, noun phrases). Writes
tests/vectors/encounter.json.

    python tests/oracle/encounter_vectors.py SNAPSHOT[:TICKS][:CLICKX:CLICKY] ...

A run with CLICKX/CLICKY clicks that point (e.g. "Continue on") whenever a
dialog has just opened, so long journeys keep going.
"""
from __future__ import annotations

import json
import pickle
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DS = 0x3364 - 0x1000 + LOAD_SEG
CHAR_BASE, CHAR_SIZE = 0x2C5E, 0x38
LOC_BASE, LOC_LEN = 0x40B6, 0x19 * 0x3D


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


SENTINEL = (0xF000, 0x0500)


def record(snap: str, ticks: int, click: tuple[int, int] | None,
           pairs: list[tuple[int, int]] | None = None) -> list[dict]:
    m = DosMachine()
    m.load("EXCAL.EXE")
    base = pickle.loads((OUT / f"{snap}.snap").read_bytes())
    m.restore(base)
    calls: list[dict] = []
    cur: dict = {}

    def view(seg: int, off: int):
        if (seg, off) == (0, 0):
            return None
        x, y = struct.unpack("<hh", m.read(seg, off + 0x12, 2) + m.read(seg, off + 0x10, 2))
        return [x, y, m.read(seg, off + 0x0E, 1)[0]]

    def chars(*ids: int) -> dict:
        return {str(i): m.read(DS, CHAR_BASE + i * CHAR_SIZE, CHAR_SIZE).hex() for i in ids}

    def entry(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        a, b = m.u16(ss, sp + 4) & 0xFF, m.u16(ss, sp + 6) & 0xFF
        cur.clear()
        cur.update(a=a, b=b, chars=chars(a, b),
                   locations=m.read(DS, LOC_BASE, LOC_LEN).hex(),
                   line=m.read(DS, 0x1E8A, 1)[0],
                   clock=m.u16(DS, 0x4FBF) | m.u16(DS, 0x4FC1) << 16,
                   views={}, rng=[], combat=None, buttons=[], phrases=[])

    def lookup(slot: str):
        def h(uc, addr, size, user):
            cur["views"].setdefault(slot, view(m.r("dx"), m.r("ax")))
        return h

    def rng(uc, addr, size, user):
        cur["rng"].append(m.r("ax"))

    def combat(uc, addr, size, user):
        cur["combat"] = m.r("ax") & 0xFF

    def button(name: str):
        def h(uc, addr, size, user):
            cur["buttons"].append(name)
        return h

    def phrase(uc, addr, size, user):
        cur["phrases"].append(m.asciiz(m.r("dx"), m.r("ax")))

    def dialog(uc, addr, size, user):
        bp = m.r("bp")
        ss = m.r("ss")
        cur["height"] = m.u16(ss, (bp - 0x48) & 0xFFFF)
        cur["choice"] = m.read(ss, (bp - 1) & 0xFFFF, 1)[0]
        cur["prefix"] = [m.asciiz(DS, m.u16(DS, 0x79E0 + 4 * i)) for i in range(4)]

    def ret(uc, addr, size, user):
        if not cur:
            return
        cur["ret"] = m.r("ax") & 0xFF
        cur["final"] = [m.read(DS, 0x9DAE, 1)[0], m.read(DS, 0x9DB0, 1)[0]]
        calls.append(dict(cur))
        cur.clear()

    hooks = [
        (0x0A12, entry), (0x0A6B, lookup("a")), (0x0A85, lookup("b")),
        (0x0CAA, lookup("a2")), (0x0CC4, lookup("b2")),
        (0x0B1F, rng), (0x0C71, combat),
        (0x0E09, button("talk")), (0x0E7E, button("join")), (0x0EBA, button("continue")),
        (0x0F7E, phrase), (0x0FFE, phrase), (0x102E, dialog), (0x1066, ret),
    ]
    for off, fn in hooks:
        addr = lin(0x2920, off)
        m.uc.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr)

    if pairs is not None:
        done = {"hit": False}

        def stop(uc, addr, size, user):
            done["hit"] = True
            uc.emu_stop()

        sent = (SENTINEL[0] << 4) + SENTINEL[1]
        m.uc.hook_add(UC_HOOK_CODE, stop, begin=sent, end=sent)
        for a, b in pairs:
            m.restore(base)
            done["hit"] = False
            m.push(b)
            m.push(a)
            m.push(SENTINEL[0])
            m.push(SENTINEL[1])
            m.w("cs", 0x2920 - 0x1000 + LOAD_SEG)
            m.w("ip", 0x0A12)
            for _ in range(150):
                m.run(1)
                if done["hit"]:
                    break
            print(f"pair {a},{b} done={done['hit']} calls={len(calls)}", flush=True)
            if not done["hit"]:
                print(f"pair {a},{b} did not return")
        print(f"{snap}: {len(calls)} direct calls")
        return calls

    seen = 0
    for _ in range(ticks // 20):
        m.run(20)
        dialogs = sum(1 for c in calls if "height" in c)
        if click and dialogs > seen:
            seen = dialogs
            m.run(40)
            m.click(*click)
    print(f"{snap}: {len(calls)} calls, {sum(1 for c in calls if 'height' in c)} dialogs")
    return calls


def main(argv: list[str]) -> int:
    calls = []
    for arg in argv:
        if arg.startswith("pairs="):
            snap = arg[6:]
            pairs = [(1, b) for b in range(2, 93)] + [(b, 1) for b in range(2, 93)]
            calls += record(snap, 0, None, pairs)
            continue
        parts = arg.split(":")
        click = (int(parts[2]), int(parts[3])) if len(parts) >= 4 else None
        calls += record(parts[0], int(parts[1]) if len(parts) > 1 else 600, click)
    path = ROOT / "tests" / "vectors" / "encounter.json"
    path.write_text(json.dumps({"calls": calls}, indent=0))
    rets = {r: sum(1 for c in calls if c["ret"] == r) for r in (0, 1)}
    print(f"{len(calls)} calls, returns={rets}, wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
