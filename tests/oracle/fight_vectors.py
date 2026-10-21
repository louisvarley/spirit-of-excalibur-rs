#!/usr/bin/env python3
"""Record golden vectors for the fight check FUN_20b5_2c06.

From an emulator snapshot on the regional map, sets DS:9DAE/9DB0 to a pair
of characters (map mode DS:9DC0 = 1) and calls the original. The UI loop
(FUN_15b8_09f7) returns at once and FUN_20b5_2a30 (start the battle) is
intercepted, so each call returns. Records the DGROUP regions involved,
map entities, buttons (label and value), message formats, the battle mode
and the return value. Writes tests/vectors/fight.json.

    python tests/oracle/fight_vectors.py SNAPSHOT
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
SENTINEL = (0xF000, 0x0500)
REGIONS = [(0x2C5E, 0x5D * 0x38 + 0x38), (0x4F25, 0x32 * 3), (0x4FC5, 1),
           (0x9C4E, 0x0A), (0x9DAC, 2), (0x9DAE, 3), (0x9DC0, 1), (0x9DC3, 0x32), (0x9DF6, 1)]


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def main(argv: list[str]) -> int:
    m = DosMachine()
    m.load("EXCAL.EXE")
    base = pickle.loads((OUT / f"{argv[0]}.snap").read_bytes())
    m.restore(base)
    st = {"done": False}

    def ret_now():
        ss, sp = m.r("ss"), m.r("sp")
        m.w("ip", m.u16(ss, sp))
        m.w("cs", m.u16(ss, sp + 2))
        m.w("sp", sp + 4)

    def stop(uc, addr, size, user):
        st["done"] = True
        uc.emu_stop()

    def skip_modal(uc, addr, size, user):
        ret_now()

    def battle(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        st["battle"] = m.u16(ss, sp + 4)
        ret_now()

    def button(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        label = m.asciiz(m.u16(ss, sp + 0x0A), m.u16(ss, sp + 0x08), 40)
        st["buttons"].append([label, m.u16(ss, sp + 0x10)])

    def message(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        st["messages"].append(m.u16(ss, sp + 0x12))

    sent = (SENTINEL[0] << 4) + SENTINEL[1]
    for addr, fn in ((sent, stop), (lin(0x15B8, 0x09F7), skip_modal),
                     (lin(0x20B5, 0x2A30), battle), (lin(0x15B8, 0x0461), button),
                     (lin(0x1772, 0x000E), message)):
        m.uc.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr)

    def regions() -> dict:
        return {f"{a:04x}": m.read(DS, a, n).hex() for a, n in REGIONS}

    ents = []
    for i in range(m.read(DS, 0x969D, 1)[0]):
        seg, off = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
        eseg, eoff = m.u16(seg, off + 2), m.u16(seg, off)
        if m.read(eseg, eoff + 0x0F, 1)[0] == 0xD3:
            ents.append((eseg, eoff))

    armies = [c for c in range(1, 93) if m.u16(DS, 0x2C5E + c * 0x38 + 0x16) & 0x400]
    pairs = [(1, b) for b in range(2, 93)] + [(b, 1) for b in range(2, 93)]
    pairs += [(a, b) for a in armies for b in range(2, 93, 5) if a != b]
    calls = []
    for a, b in pairs:
        m.restore(base)
        m.write(DS, 0x9DAE, bytes([a]))
        m.write(DS, 0x9DB0, bytes([b]))
        m.write(DS, 0x9DC0, bytes([1]))
        st.update(done=False, buttons=[], messages=[], battle=None)
        before = {"regions": regions(), "entities": [m.read(s, o, 0x36).hex() for s, o in ents]}
        m.push(SENTINEL[0])
        m.push(SENTINEL[1])
        m.w("cs", 0x20B5 - 0x1000 + LOAD_SEG)
        m.w("ip", 0x2C06)
        m.w("flags", m.r("flags") | 0x200)
        for _ in range(1500):
            m.run(1)
            if st["done"]:
                break
        if not st["done"]:
            cs, ip = m.r("cs"), m.r("ip")
            print(f"pair {a},{b}: did not return, at {cs - LOAD_SEG + 0x1000:04x}:{ip:04x}", flush=True)
            continue
        calls.append({
            "pair": [a, b], "before": before,
            "after": {"regions": regions(), "entities": [m.read(s, o, 0x36).hex() for s, o in ents]},
            "ret": m.r("ax") & 0xFF, "buttons": st["buttons"], "messages": st["messages"],
            "battle": st["battle"],
        })
    out = {"dgroup": m.read(DS, 0, 0x10000).hex(), "calls": calls}
    path = ROOT / "tests" / "vectors" / "fight.json"
    path.write_text(json.dumps(out))
    fights = sum(1 for c in calls if c["ret"])
    modes = sorted({c["battle"] for c in calls if c["battle"] is not None})
    print(f"{len(calls)} calls ({len(armies)} armies), {fights} confrontations, modes {modes}; wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
