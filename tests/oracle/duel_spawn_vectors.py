#!/usr/bin/env python3
"""Record the character spawns of FUN_2d65_13e2 as a single combat starts.

Sets up the duel as duel_watch.py does, then, when the scene builder
returns from the room set-up (2d65:1767), saves DGROUP, the room base (AL)
and the scene mode bits ([BP-1]). It then records the random numbers drawn
and each character widget created (returns at 2d65:18a8 and 2d65:1ae7),
with its sprite-sheet header, reading the widgets at 2d65:1b68. The
callback segment at +0x08 is a runtime segment (LOAD_SEG-relative).
Writes tests/vectors/duel_spawn.json.

    python tests/oracle/duel_spawn_vectors.py [SNAPSHOT A B TICKS]
"""
from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from duel_watch import DS, lin, setup_duel  # noqa: E402
from play import OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def main(argv: list[str]) -> int:
    snap, a, b, ticks = (argv + ["map", "1", "62", "600"][len(argv):])[:4]
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    setup_duel(m, int(a), int(b))
    runs: list[dict] = []
    cur: dict | None = None

    def start(uc, addr, size, user):
        nonlocal cur
        cur = {"ds": m.read(DS, 0, 0x10000).hex(), "al": m.r("ax") & 0xFF,
               "mode": m.read(m.r("ss"), m.r("bp") - 1, 1)[0], "rng": [], "ptrs": [],
               "sheets": {}}

    def rng(uc, addr, size, user):
        if cur is not None:
            cur["rng"].append(m.r("ax"))

    def spawned(uc, addr, size, user):
        if cur is None:
            return
        ns, no = m.r("dx"), m.r("ax")
        ws, wo = m.u16(ns, no + 2), m.u16(ns, no)
        wd = m.read(ws, wo, 0x36)
        img = int.from_bytes(wd[2:6], "little")
        if img:
            hs, ho = m.u16(img >> 16, (img & 0xFFFF) + 2), m.u16(img >> 16, img & 0xFFFF)
            count = m.read(hs, ho + 3, 1)[0]
            cur["sheets"][str(img)] = m.read(hs, ho, 4 + 6 * max(count, 1) + 6).hex()
        cur["ptrs"].append((ws, wo))

    def finish(uc, addr, size, user):
        nonlocal cur
        if cur is not None:
            # Read the widgets now: the builder sets +0x27/+0x34/+0x30
            # after FUN_15b8_0461 returns.
            cur["spawned"] = [m.read(s, o, 0x36).hex() for s, o in cur.pop("ptrs")]
            runs.append(cur)
            cur = None

    hook = lambda f, seg, off: m.uc.hook_add(UC_HOOK_CODE, f, begin=lin(seg, off), end=lin(seg, off))
    hook(start, 0x2D65, 0x1767)
    hook(rng, 0x15B8, 0x182E)
    hook(spawned, 0x2D65, 0x18A8)
    hook(spawned, 0x2D65, 0x1AE7)
    hook(finish, 0x2D65, 0x1B68)
    m.run(int(ticks))
    print(f"{len(runs)} scene builds, spawned {[len(r['spawned']) for r in runs]}")
    path = ROOT / "tests" / "vectors" / "duel_spawn.json"
    path.write_text(json.dumps({"runs": runs}))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
