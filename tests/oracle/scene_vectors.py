#!/usr/bin/env python3
"""Record golden vectors for the per-widget script step FUN_18cb_066a.

For every call while the original plays from a snapshot, records the whole
widget list (DS:969D/969F) and the scene globals before and after, the clock
at each loop check, the random numbers drawn, the sounds started, the images
loaded, and whether the talk/combat hooks ran. Scripts (DS:1E2A) and sprite
sheet headers are stored once per snapshot.
Writes tests/vectors/scene_steps.json. With --frames, records whole scene
frames (FUN_18cb_0002) instead, with the clock captured at every read the
port makes, and writes tests/vectors/scene_frames.json.

    python tests/oracle/scene_vectors.py [--frames] SNAPSHOT[:TICKS] ...
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
PER_SNAP = 1500   # calls kept per snapshot


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def record(snap: str, ticks: int, frames: bool = False) -> dict:
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    calls: list[dict] = []
    programs: dict[int, str] = {}
    sheets: dict[int, str] = {}
    cur: dict | None = None

    def clock() -> int:
        return m.u16(DS, 0x7E3E) | m.u16(DS, 0x7E40) << 16

    def widgets() -> list[str]:
        out = []
        for i in range(m.read(DS, 0x969D, 1)[0]):
            s, o = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
            ws, wo = m.u16(s, o + 2), m.u16(s, o)
            wd = m.read(ws, wo, WIDGET)
            out.append(wd.hex())
            img = int.from_bytes(wd[2:6], "little")
            if img and img not in sheets:
                hs, ho = m.u16(img >> 16, (img & 0xFFFF) + 2), m.u16(img >> 16, img & 0xFFFF)
                count = m.read(hs, ho + 3, 1)[0]
                sheets[img] = m.read(hs, ho, 4 + 6 * max(count, 1) + 6).hex()
        return out

    def globals_() -> dict:
        return {
            "current": m.u16(DS, 0x9A6C), "word_9a76": m.u16(DS, 0x9A76),
            "bits_9a78": m.read(DS, 0x9A78, 1)[0], "flag_01f5": m.read(DS, 0x01F5, 1)[0],
            "word_9a79": m.u16(DS, 0x9A79),
            "clip": [m.u16(DS, 0x03C9 + 2 * k) for k in range(4)],
        }

    def snapshot_programs():
        for n in range(64):
            if n in programs:
                continue
            s, o = m.u16(DS, 0x1E2A + 4 * n + 2), m.u16(DS, 0x1E2A + 4 * n)
            if s or o:
                programs[n] = m.read(s, o, 0x200).hex()

    def enter(uc, addr, size, user):
        nonlocal cur
        snapshot_programs()
        cur = {"before": widgets(), "globals": globals_(), "clocks": [], "rng": [],
               "sounds": [], "loads": [], "hooked": False, "clock0": clock()}

    def loop_check(uc, addr, size, user):
        if cur is not None:
            cur["clocks"].append(clock())

    def rng(uc, addr, size, user):
        if cur is not None:
            cur["rng"].append(m.r("ax"))

    def sound(kind):
        def h(uc, addr, size, user):
            if cur is not None:
                cur["sounds"].append([kind, m.u16(m.r("ss"), m.r("sp") + 4)])
        return h

    def loaded(uc, addr, size, user):
        if cur is not None:
            cur["loads"].append(m.r("dx") << 16 | m.r("ax"))

    def hooked(uc, addr, size, user):
        if cur is not None:
            cur["hooked"] = True

    def leave(uc, addr, size, user):
        nonlocal cur
        if cur is None:
            return
        cur["after"] = widgets()
        cur["globals_after"] = globals_()
        if len(calls) < PER_SNAP:
            calls.append(cur)
        cur = None

    hook = lambda f, seg, off: m.uc.hook_add(UC_HOOK_CODE, f, begin=lin(seg, off), end=lin(seg, off))
    if frames:
        hook(enter, 0x18CB, 0x0002)
        for off in (0x002B, 0x066A, 0x072B, 0x05CA):
            hook(loop_check, 0x18CB, off)
    else:
        hook(enter, 0x18CB, 0x066A)
        hook(loop_check, 0x18CB, 0x072B)
    hook(rng, 0x15B8, 0x182E)
    hook(sound("music"), 0x1316, 0x181D)
    hook(sound("sound"), 0x1316, 0x183C)
    hook(sound("cue"), 0x1316, 0x1828)
    hook(loaded, 0x18CB, 0x1418)
    hook(hooked, 0x26C5, 0x071A)
    hook(hooked, 0x20B5, 0x1DFF)
    hook(leave, 0x18CB, 0x05AF if frames else 0x08FE)
    m.run(ticks)
    print(f"{snap}: {len(calls)} steps, {len(programs)} scripts, {len(sheets)} sheets", flush=True)
    return {"snap": snap, "programs": programs, "sheets": {str(k): v for k, v in sheets.items()},
            "calls": calls}


def main(argv: list[str]) -> int:
    frames = "--frames" in argv
    runs = []
    for arg in argv:
        if arg.startswith("--"):
            continue
        name, _, ticks = arg.partition(":")
        runs.append(record(name, int(ticks or 600), frames))
    path = ROOT / "tests" / "vectors" / ("scene_frames.json" if frames else "scene_steps.json")
    path.write_text(json.dumps({"runs": runs}))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
