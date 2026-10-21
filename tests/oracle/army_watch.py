#!/usr/bin/env python3
"""Start an army battle in the original and watch it.

No army stands on the map in the snapshots, so this sets the battle up the
way the fight check leaves it (side lists DS:9DC3/9DD7 and counts
DS:9DAC/9DAD, leaders DS:9DAE/9DB0, champions DS:9C4E/9C4F, map mode
DS:9DC0) and calls FUN_20b5_2a30(0x10), which runs FUN_2449_134e. Saves
screenshots (army*.png) and a snapshot once the battle screen is up
(army.snap).

    python tests/oracle/army_watch.py SNAPSHOT SIDE1 SIDE2 [TICKS] [KEYS]

SIDE1/SIDE2 are comma-separated character ids, leader first; KEYS is a
comma-separated list of key presses spread over the run.
"""
from __future__ import annotations

import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

DS = 0x3364 - 0x1000 + LOAD_SEG
SENTINEL = (0xF000, 0x0500)


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def start_army_battle(m: DosMachine, side1: list[int], side2: list[int], mode: int = 0x10) -> dict:
    """Set up the battle lists and enter FUN_20b5_2a30(mode). Returns a
    dict whose "done" turns True when 2a30 returns."""
    m.write(DS, 0x9DC3, bytes(0x32))
    m.write(DS, 0x9DC3, bytes(side1))
    m.write(DS, 0x9DD7, bytes(side2))
    m.write(DS, 0x9DAC, bytes([len(side1), len(side2)]))
    m.write(DS, 0x9DAE, bytes([side1[0]]))
    m.write(DS, 0x9DB0, bytes([side2[0]]))
    m.write(DS, 0x9C4E, bytes([side1[0], side2[0]]))
    m.write(DS, 0x9C52, bytes([0, 0x14]))
    m.write(DS, 0x9DC0, bytes([1]))
    # Stand side two next to side one's leader, as on the map.
    x, y = m.u16(DS, 0x2C6C + side1[0] * 0x38), m.u16(DS, 0x2C6E + side1[0] * 0x38)
    for c in side2:
        m.put16(DS, 0x2C6C + c * 0x38, x + 8)
        m.put16(DS, 0x2C6E + c * 0x38, y)
    st = {"done": False, "hook": None}
    # Enter 2a30 from the map loop (FUN_1b06_0007), not from wherever the
    # snapshot stopped (often inside the timer interrupt), and resume that
    # call once the battle returns.
    step = lin(0x1B06, 0x0007)
    sent = (SENTINEL[0] << 4) + SENTINEL[1]

    def enter(uc, addr, size, user):
        if st["hook"] is None:
            return
        m.uc.hook_del(st["hook"])
        st["hook"] = None
        m.push(mode)
        m.push(SENTINEL[0])
        m.push(SENTINEL[1])
        m.w("cs", 0x20B5 - 0x1000 + LOAD_SEG)
        m.w("ip", 0x2A30)
        uc.emu_stop()

    def back(uc, addr, size, user):
        st["done"] = True
        m.pop()
        m.w("cs", 0x1B06 - 0x1000 + LOAD_SEG)
        m.w("ip", 0x0007)
        uc.emu_stop()

    st["hook"] = m.uc.hook_add(UC_HOOK_CODE, enter, begin=step, end=step)
    m.uc.hook_add(UC_HOOK_CODE, back, begin=sent, end=sent)
    return st


def main(argv: list[str]) -> int:
    snap = argv[0]
    side1 = [int(x) for x in argv[1].split(",")]
    side2 = [int(x) for x in argv[2].split(",")]
    ticks = int(argv[3]) if len(argv) > 3 else 800
    keys = argv[4].split(",") if len(argv) > 4 else []
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    st = start_army_battle(m, side1, side2)
    step = max(1, ticks // 8)
    for i in range(8):
        if i < len(keys) and keys[i]:
            m.press(keys[i])
        m.run(step)
        m.save_png(OUT / f"army{i}.png")
        print(f"t={m.ticks} 9c56={m.u16(DS, 0x9C56):04x} cs:ip={m.r('cs') - LOAD_SEG + 0x1000:04x}:{m.r('ip'):04x}"
              f" slots {m.read(DS, 0x9C82, 4).hex()}/{m.read(DS, 0x9C96, 4).hex()} done={st['done']}", flush=True)
        if i == 1:
            (OUT / "army.snap").write_bytes(pickle.dumps(m.snapshot()))
        if st["done"]:
            break
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
