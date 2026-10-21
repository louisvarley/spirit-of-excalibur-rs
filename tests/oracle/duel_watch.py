#!/usr/bin/env python3
"""Start a single combat in the original and watch it.

From a map snapshot, calls the fight check FUN_20b5_2c06 for a pair, accepts
the challenge (the first UI wait returns at once, leaving the mode as set),
then lets FUN_20b5_2a30 run the combat normally. Saves screenshots and a
function trace, and a snapshot once the combat screen is up.

    python tests/oracle/duel_watch.py SNAPSHOT A B [TICKS]
"""
from __future__ import annotations

import pickle
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from play import LOAD_SEG, OUT, function_entries  # noqa: E402
from unicorn import UC_HOOK_BLOCK, UC_HOOK_CODE  # noqa: E402

DS = 0x3364 - 0x1000 + LOAD_SEG


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def setup_duel(m: DosMachine, a: int, b: int) -> tuple[int, int, int, int]:
    """Place B's entity next to A's on the map so they meet, and make the
    first UI wait inside the fight check return at once (accepting the
    challenge). Returns the two entities' (segment, offset) pairs."""
    # Put B's map entity right next to A's, as if they had just met; the
    # game's own collision scan then runs the encounter and fight checks.
    ents = {}
    for i in range(m.read(DS, 0x969D, 1)[0]):
        seg, off = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
        eseg, eoff = m.u16(seg, off + 2), m.u16(seg, off)
        if m.read(eseg, eoff + 0x0F, 1)[0] == 0xD3:
            ents[m.u16(eseg, eoff + 0x0A)] = (eseg, eoff)
    (sa, oa), (sb, ob) = ents[a], ents[b]
    ax, ay = m.u16(sa, oa + 0x12), m.u16(sa, oa + 0x10)
    import struct
    # B waits 20 px away; A walks onto B's exact pixel (radius is 1 for
    # movement class 0).
    bx, by = ax + 20, ay
    for off, v in ((0x12, bx), (0x10, by), (0x22, bx), (0x20, by), (0x1E, 0), (0x1C, 0)):
        m.write(sb, ob + off, struct.pack("<H", v))
    for off, v in ((0x1E, bx + 20), (0x1C, by), (0x22, ax), (0x20, ay)):
        m.write(sa, oa + off, struct.pack("<H", v))
    # Collision radius 16 (movement class 2) on B's entity only, so the
    # meeting does not depend on landing on the exact pixel.
    m.write(sb, ob + 0x26, bytes([(m.read(sb, ob + 0x26, 1)[0] & 0xCF) | 0x20]))
    # Move everyone else near A out of the way so only A and B meet.
    for cid, (sx, ox) in ents.items():
        if cid in (a, b):
            continue
        x, y = m.u16(sx, ox + 0x12), m.u16(sx, ox + 0x10)
        if abs(x - ax) < 96 and abs(y - ay) < 96:
            for off in (0x12, 0x22, 0x1E):
                m.write(sx, ox + off, struct.pack("<H", (ax + 300) & 0xFFFF))
            m.write(sx, ox + 0x1E, struct.pack("<H", 0))
    # Encounters are ignored before clock 0xA00 (FUN_2920_0a12).
    if m.u16(DS, 0x4FBF) < 0xA00 and m.u16(DS, 0x4FC1) == 0:
        m.write(DS, 0x4FBF, struct.pack("<H", 0x1000))
    fight_lo, fight_hi = lin(0x20B5, 0x2C06), lin(0x20B5, 0x3949)
    skips = {"n": 0}

    def skip_first_modal(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        ret = (m.u16(ss, sp + 2) << 4) + m.u16(ss, sp)
        if skips["n"] == 0 and fight_lo <= ret <= fight_hi:
            skips["n"] += 1
            m.w("ip", m.u16(ss, sp))
            m.w("cs", m.u16(ss, sp + 2))
            m.w("sp", sp + 4)

    m.uc.hook_add(UC_HOOK_CODE, skip_first_modal, begin=lin(0x15B8, 0x09F7), end=lin(0x15B8, 0x09F7))
    return sa, oa, sb, ob


def main(argv: list[str]) -> int:
    snap, a, b = argv[0], int(argv[1]), int(argv[2])
    ticks = int(argv[3]) if len(argv) > 3 else 600
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    sa, oa, sb, ob = setup_duel(m, a, b)
    seen = []

    def scan(uc, addr, size, user):
        if len(seen) < 12:
            seen.append((m.u16(sa, oa + 0x12), m.u16(sa, oa + 0x10), m.read(sa, oa + 0x0E, 1)[0],
                         m.u16(sb, ob + 0x12), m.u16(sb, ob + 0x10), m.read(sb, ob + 0x0E, 1)[0],
                         m.read(sb, ob + 0x26, 1)[0], m.u16(sb, ob + 0x1E)))

    m.uc.hook_add(UC_HOOK_CODE, scan, begin=lin(0x1B06, 0x079C), end=lin(0x1B06, 0x079C))
    names = function_entries()
    hits: Counter = Counter()
    m.uc.hook_add(UC_HOOK_BLOCK, lambda uc, addr, size, u: hits.update([names[addr]]) if addr in names else None)
    step = max(1, ticks // 8)
    for i in range(8):
        m.run(step)
        m.save_png(OUT / f"duel{i}.png")
        print(f"t={m.ticks} 9c56={m.u16(DS, 0x9C56):04x} cs:ip={m.r('cs') - LOAD_SEG + 0x1000:04x}:{m.r('ip'):04x}", flush=True)
    print("scan:", seen)
    print("radius table", m.read(DS, 0x067E, 4).hex())
    (OUT / "duel.snap").write_bytes(pickle.dumps(m.snapshot()))
    for name, n in hits.most_common(40):
        print(f"{n:8d}  {name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
