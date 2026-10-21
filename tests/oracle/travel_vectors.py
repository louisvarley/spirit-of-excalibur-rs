#!/usr/bin/env python3
"""Record golden vectors for FUN_1b06_0007 (map entity travel step).

Runs the original game from an emulator snapshot taken on the regional map
and hooks every real call of 1b06:0007, recording the entity bytes, clock and
scratch globals before the call and after it (or at the callback for calls
that end in the arrival/wake callbacks). Writes tests/vectors/travel_step.json.

    python tests/oracle/travel_vectors.py SNAPSHOT[:TICKS] ...
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
ENTITY_BYTES = 0x36


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def main(argv: list[str]) -> int:
    runs = [record(*(a.split(":") + ["600"])[:2]) for a in argv]
    out = runs[0]
    out["source"] = [r["source"] for r in runs]
    out["calls"] = [c for r in runs for c in r["calls"]]
    records = out["calls"]
    path = ROOT / "tests" / "vectors" / "travel_step.json"
    path.write_text(json.dumps(out))
    moved = sum(1 for r in records if r["before"] != r["after"])
    ends = {k: sum(1 for r in records if r["end"] == k) for k in ("return", "wake", "arrive")}
    print(f"{len(records)} calls, {moved} changed the entity, ends={ends}")
    print("wrote", path)
    return 0


def record(snap: str, ticks: str) -> dict:
    ticks = int(ticks)
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))

    def u16(off: int) -> int:
        return m.u16(DS, off)

    def far(off: int) -> tuple[int, int]:
        return m.u16(DS, off + 2), m.u16(DS, off)

    def entity_ptr(index: int) -> tuple[int, int]:
        seg, off = far(0x969F + index * 4)
        return m.u16(seg, off + 2), m.u16(seg, off)

    def scratch() -> dict:
        return {"dir": m.read(DS, 0x9B25, 1)[0],
                "best_tile_x": struct.unpack("<h", m.read(DS, 0x9A7C, 2))[0],
                "best_tile_y": struct.unpack("<h", m.read(DS, 0x9A7E, 2))[0]}

    records: list[dict] = []
    cur: dict = {}

    def on_entry(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        index = m.u16(ss, sp + 4) & 0xFF
        seg, off = entity_ptr(index)
        cur.clear()
        cur.update(index=index, ptr=(seg, off),
                   before=m.read(seg, off, ENTITY_BYTES).hex(),
                   clock=u16(0x4FBF) | u16(0x4FC1) << 16,
                   wake_callback=bool(u16(0x676) | u16(0x678)),
                   scratch_before=scratch(), follow=None, woke=None, arrived=False)

    def on_follow(uc, addr, size, user):
        dx, ax = m.r("dx"), m.r("ax")
        if (dx, ax) != (0, 0):
            cur["follow"] = [struct.unpack("<h", m.read(dx, ax + 0x12, 2))[0],
                             struct.unpack("<h", m.read(dx, ax + 0x10, 2))[0]]

    def finish(kind: str):
        seg, off = cur["ptr"]
        cur["after"] = m.read(seg, off, ENTITY_BYTES).hex()
        cur["scratch_after"] = scratch()
        cur["end"] = kind
        rec = dict(cur)
        rec.pop("ptr")
        records.append(rec)

    def on_wake(uc, addr, size, user):
        seg, off = cur["ptr"]
        cur["woke"] = m.u16(seg, off + 0x0A)
        finish("wake")        # callback may change the entity; stop comparing here

    def on_arrive(uc, addr, size, user):
        cur["arrived"] = True
        finish("arrive")

    def on_ret(uc, addr, size, user):
        if cur and "end" not in cur:
            finish("return")

    hooks = [
        (lin(0x1B06, 0x0007), on_entry),
        (lin(0x1B06, 0x00A2), on_follow),
        (lin(0x1B06, 0x0076), on_wake),
        (lin(0x1B06, 0x0151), on_arrive),
        (lin(0x1B06, 0x06B4), on_ret),
    ]
    for addr, fn in hooks:
        m.uc.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr)

    # Terrain grid behind FUN_15b8_1b1b: [DS:8E29] -> +0x44 width, +0x38 -> -> tiles.
    mseg, moff = far(0x8E29)
    width = m.u16(mseg, moff + 0x44)
    pseg, poff = m.u16(mseg, moff + 0x3A), m.u16(mseg, moff + 0x38)
    tseg, toff = m.u16(pseg, poff + 2), m.u16(pseg, poff)
    tiles = m.read(tseg, toff, width * 61)

    m.run(ticks)
    print(f"{snap}: {len(records)} calls")
    return {
        "source": snap,
        "dgroup": m.read(DS, 0, 0x10000).hex(),
        "tile_width": width,
        "tiles": tiles.hex(),
        "calls": records,
    }


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
