#!/usr/bin/env python3
"""Record golden vectors for the 1ba8 inventory leaves.

Calls the original functions directly from emulator snapshots:

- FUN_1ba8_0431(def, from, to, count)   transfer instances / gold
- FUN_1ba8_04fd(def, owner)             count carried instances / gold
- FUN_1ba8_2ad2(def, owner, bit, class) find a carried instance
- FUN_1ba8_0068(id)                     "You can't do that with a statue"
- FUN_1ba8_01ff(def, owner)             a carried item falls to the ground

For each call it records the arguments, AL/AX, the random numbers drawn
(FUN_15b8_17e0), the message boxes opened (FUN_1772_000e is skipped, its
string offset recorded) and every DGROUP byte that changed outside the stack.
Writes tests/vectors/inventory.json.

    python tests/oracle/inventory_vectors.py [SNAPSHOT ...]
"""
from __future__ import annotations

import json
import os
import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import GAME_DIR, DosMachine  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DS = 0x3364 - 0x1000 + LOAD_SEG
SENTINEL = (0xF000, 0x0500)
INST, NINST = 0x4DD1, 0x55


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def game_dir() -> Path:
    env = os.environ.get("SOE_GAME")
    if env:
        return Path(env)
    if (GAME_DIR / "EXCAL.EXE").exists():
        return GAME_DIR
    # A worktree: fall back to the main checkout's game/.
    for p in ROOT.parents:
        if (p / "game" / "EXCAL.EXE").exists():
            return p / "game"
    return GAME_DIR


def record(snap: str) -> dict:
    m = DosMachine(game_dir=game_dir())
    m.load("EXCAL.EXE")
    base = pickle.loads((OUT / f"{snap}.snap").read_bytes())
    m.restore(base)
    ds0 = m.read(DS, 0, 0x10000)
    cur: dict = {}

    def stop(uc, addr, size, user):
        cur["done"] = True
        uc.emu_stop()

    def rng(uc, addr, size, user):
        cur["rng"].append(m.r("ax"))

    def retf() -> None:
        ss, sp = m.r("ss"), m.r("sp")
        m.w("ip", m.u16(ss, sp))
        m.w("cs", m.u16(ss, sp + 2))
        m.w("sp", sp + 4)

    def msgbox(uc, addr, size, user):
        # Skip the dialog: record the format string (arg 8) with its
        # arguments decoded (%S/%s strings, %d signed, %c/%W raw words).
        ss, sp = m.r("ss"), m.r("sp")
        arg = lambda k: m.u16(ss, sp + 4 + 2 * k)
        text = arg(7)
        fmt = m.asciiz(arg(8), text, 256)
        k, out, i = 9, [], 0
        while i < len(fmt) - 1:
            if fmt[i] == "%":
                c = fmt[i + 1]
                i += 1
                if c in "Ss":
                    out.append(m.asciiz(arg(k + 1), arg(k), 256))
                    k += 2
                elif c == "d":
                    out.append(arg(k) - 0x10000 if arg(k) & 0x8000 else arg(k))
                    k += 1
                elif c in "cW":
                    out.append(arg(k))
                    k += 1
            i += 1
        cur["msgs"].append([text, out])
        retf()

    def skip(name: str, nargs: int):
        def h(uc, addr, size, user):
            ss, sp = m.r("ss"), m.r("sp")
            cur["subcalls"].append([name] + [m.u16(ss, sp + 4 + 2 * k) for k in range(nargs)])
            retf()
        return h

    hook = lambda f, a: m.uc.hook_add(UC_HOOK_CODE, f, begin=a, end=a)
    hook(stop, (SENTINEL[0] << 4) + SENTINEL[1])
    hook(rng, lin(0x15B8, 0x182E))
    hook(msgbox, lin(0x1772, 0x000E))
    # Not ported with these leaves: recorded and skipped.
    hook(skip("15b8_1876", 1), lin(0x15B8, 0x1876))
    hook(skip("1772_0578", 0), lin(0x1772, 0x0578))
    hook(skip("20b5_1229", 1), lin(0x20B5, 0x1229))

    def call(off: int, args: list[int], patch: list[list[int]] = ()) -> dict:
        m.restore(base)
        for at, v in patch:
            m.write(DS, at, bytes([v]))
        cur.clear()
        cur.update(done=False, rng=[], msgs=[], subcalls=[])
        for v in reversed(args):
            m.push(v)
        m.push(SENTINEL[0])
        m.push(SENTINEL[1])
        sp = ((m.r("ss") - DS) << 4) + m.r("sp")
        m.w("cs", 0x1BA8 - 0x1000 + LOAD_SEG)
        m.w("ip", off)
        m.w("flags", m.r("flags") | 0x200)
        for _ in range(50):
            m.run(1)
            if cur["done"]:
                break
        else:
            raise RuntimeError(f"1ba8:{off:04x}{args} did not return")
        after = m.read(DS, 0, 0x10000)
        lo, hi = sp - 0x800, sp + 0x40
        before = bytearray(ds0)
        for at, v in patch:
            before[at] = v
        diff = [[i, after[i]] for c in range(0, 0x10000, 0x100) if after[c:c + 0x100] != before[c:c + 0x100]
                for i in range(c, c + 0x100) if after[i] != before[i] and not lo <= i < hi]
        return {"fn": f"{off:04x}", "args": args, "patch": [list(p) for p in patch], "ax": m.r("ax"), "rng": cur["rng"],
                "msgs": cur["msgs"], "subcalls": cur["subcalls"], "writes": diff}

    calls = []
    carried = []
    for i in range(NINST):
        d, o, c, f = ds0[INST + 4 * i:INST + 4 * i + 4]
        if d and (f & 0x18) == 0x18:
            carried.append((i, d, o, c, f))
    owners = sorted({o for _, _, o, _, _ in carried} | {1, 2, 3})
    defs = sorted({d for _, d, _, _, _ in carried} | {1})
    for cid in range(0, 93):
        calls.append(call(0x0068, [cid]))
        # No statues stand in the snapshots: make one.
        calls.append(call(0x0068, [cid], [[0x2C5E + cid * 0x38 + 0x26, 0x16]]))
    for o in owners:
        for d in defs:
            calls.append(call(0x04FD, [d, o]))
            for bit in (0, 1):
                for cls in (0, 1, 2, 5):
                    calls.append(call(0x2AD2, [d, o, bit, cls]))
            calls.append(call(0x01FF, [d, o]))
            for to in (owners[0], o + 1):
                for n in (1, 2, 7):
                    calls.append(call(0x0431, [d, o, to, n]))
    for cid in range(1, 93):
        for stat in (0x25, 0x2D, 0x2F, 0x30):
            for delta in (-60, -3, -1, 1, 4, 250):
                calls.append(call(0x2611, [stat, cid, delta & 0xFFFF]))
        # Low nobility: a loss makes a knight forswear the Round Table.
        calls.append(call(0x2611, [0x2F, cid, 0xFFFE], [[0x2C5E + cid * 0x38 + 0x2F, 3]]))
    print(f"{snap}: {len(carried)} carried instances, {len(calls)} calls", flush=True)
    return {"snapshot": snap, "dgroup": ds0.hex(), "calls": calls}


def main(argv: list[str]) -> int:
    snaps = [record(s) for s in argv or ["hall", "york0", "roundtable"]]
    path = ROOT / "tests" / "vectors" / "inventory.json"
    path.write_text(json.dumps({"snapshots": snaps}))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
