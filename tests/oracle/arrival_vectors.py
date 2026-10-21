#!/usr/bin/env python3
"""Record golden vectors for the arrival callback FUN_2920_026c.

From an emulator snapshot on the regional map, puts map entities on
locations (as if they had just arrived) and calls the original callback with
the entity's widget index. The modal wait after the "reached" message
(FUN_15b8_09f7) is skipped wherever it is called. Records the DGROUP regions
it reads/writes, all map entities, the message windows (format + string
arguments) and the loop word. Writes tests/vectors/arrival.json.

    python tests/oracle/arrival_vectors.py SNAPSHOT [LOCATIONS_PER_ENTITY]
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
SENTINEL = (0xF000, 0x0500)
REGIONS = [(0x01EC, 2), (0x2C5E, 0x5D * 0x38 + 0x38), (0x40B6, 0x3D * 0x19), (0x4FCE, 4)]


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def main(argv: list[str]) -> int:
    per_entity = int(argv[1]) if len(argv) > 1 else 6
    m = DosMachine()
    m.load("EXCAL.EXE")
    base = pickle.loads((OUT / f"{argv[0]}.snap").read_bytes())
    m.restore(base)
    state = {"done": False, "messages": []}

    def stop(uc, addr, size, user):
        state["done"] = True
        uc.emu_stop()

    def skip_modal(uc, addr, size, user):
        # The UI loop waits for the player; return from it at once.
        ss, sp = m.r("ss"), m.r("sp")
        m.w("ip", m.u16(ss, sp))
        m.w("cs", m.u16(ss, sp + 2))
        m.w("sp", sp + 4)

    def message(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        fmt = m.asciiz(m.u16(ss, sp + 0x14), m.u16(ss, sp + 0x12), 200)
        args = []
        for k in range(fmt.count("%S") + fmt.count("%s")):
            off, seg = m.u16(ss, sp + 0x16 + 4 * k), m.u16(ss, sp + 0x18 + 4 * k)
            args.append(m.asciiz(seg, off, 120))
        state["messages"].append({"format": fmt, "args": args})

    sent = (SENTINEL[0] << 4) + SENTINEL[1]
    m.uc.hook_add(UC_HOOK_CODE, stop, begin=sent, end=sent)
    m.uc.hook_add(UC_HOOK_CODE, skip_modal, begin=lin(0x15B8, 0x09F7), end=lin(0x15B8, 0x09F7))
    m.uc.hook_add(UC_HOOK_CODE, message, begin=lin(0x1772, 0x000E), end=lin(0x1772, 0x000E))

    def regions() -> dict:
        return {f"{a:04x}": m.read(DS, a, n).hex() for a, n in REGIONS}

    def entities() -> list[tuple[int, str]]:
        out = []
        for i in range(m.read(DS, 0x969D, 1)[0]):
            seg, off = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
            eseg, eoff = m.u16(seg, off + 2), m.u16(seg, off)
            if m.read(eseg, eoff + 0x0F, 1)[0] == 0xD3:
                out.append((i, eseg, eoff))
        return out

    locations = [i for i in range(1, 0x3D)
                 if m.u16(DS, 0x40B6 + 0x19 * i + 0x12) != 0]
    burnable = [i for i in locations if 600 <= m.u16(DS, 0x40B6 + 0x19 * i + 0x16) <= 607]
    calls = []
    ents = entities()
    jobs = []
    for n, (index, eseg, eoff) in enumerate(ents):
        picks = [locations[(n * 7 + k * 11) % len(locations)] for k in range(per_entity)]
        jobs += [(n, index, eseg, eoff, loc, None) for loc in picks + [0]]
    # Armies: a non-party entity with flag 0x400 and a big count, at every
    # burnable location, with strengths either side of the thresholds.
    for k, loc in enumerate(burnable):
        n, (index, eseg, eoff) = 1 + k % (len(ents) - 1), ents[1 + k % (len(ents) - 1)]
        for count in (1, 25, 160, 600):
            jobs.append((n, index, eseg, eoff, loc, count))
    for n, index, eseg, eoff, loc, army in jobs:
        if True:
            m.restore(base)
            if army is not None:
                cid = m.u16(eseg, eoff + 0x0A)
                at = 0x2C5E + cid * 0x38
                m.write(DS, at + 0x16, struct.pack("<H", m.u16(DS, at + 0x16) | 0x400))
                m.write(DS, at + 0x1E, struct.pack("<H", army))
                ring = burnable[:4] if len(burnable) > 4 else [0, 0, 0, 0]
                m.write(DS, 0x4FCE, bytes(r for r in ring))
            if loc:
                x = m.u16(DS, 0x40B6 + 0x19 * loc + 0x12)
                y = m.u16(DS, 0x40B6 + 0x19 * loc + 0x14)
            else:
                x, y = 0x123, 0x177
            for off, v in ((0x12, x), (0x10, y), (0x1E, x), (0x1C, y)):
                m.write(eseg, eoff + off, struct.pack("<H", v))
            before = {"regions": regions(),
                      "entities": [m.read(s, o, 0x36).hex() for _, s, o in ents]}
            state["done"] = False
            state["messages"] = []
            m.push(index)
            m.push(SENTINEL[0])
            m.push(SENTINEL[1])
            m.w("cs", 0x2920 - 0x1000 + LOAD_SEG)
            m.w("ip", 0x026C)
            m.w("flags", m.r("flags") | 0x200)  # snapshot may be inside an IRQ
            for _ in range(200):
                m.run(1)
                if state["done"]:
                    break
            if not state["done"]:
                cs, ip = m.r("cs"), m.r("ip")
                print(f"entity {index} loc {loc}: did not return, at "
                      f"{cs - LOAD_SEG + 0x1000:04x}:{ip:04x}, messages {state['messages']}")
                continue
            calls.append({
                "entity": n, "location": loc, "before": before,
                "after": {"regions": regions(),
                          "entities": [m.read(s, o, 0x36).hex() for _, s, o in ents]},
                "messages": list(state["messages"]),
            })
    out = {"dgroup": m.read(DS, 0, 0x10000).hex(), "calls": calls}
    path = ROOT / "tests" / "vectors" / "arrival.json"
    path.write_text(json.dumps(out))
    msgs = sum(len(c["messages"]) for c in calls)
    torch = sum(1 for c in calls for x in c["messages"] if "torch" in x["format"])
    print(f"{len(calls)} calls, {msgs} messages, {torch} torch; wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
