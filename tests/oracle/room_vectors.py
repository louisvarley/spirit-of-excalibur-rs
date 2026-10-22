#!/usr/bin/env python3
"""Record the room builder FUN_2b79_0003 in the original.

From a snapshot, runs the game (optionally setting up a single combat
between characters A and B as duel_watch.py does) and records every call
of FUN_2b79_0003: on entry DGROUP, the argument, the scene record at
DS:72E6 and the widget list; while it runs the image handles it loads
(FUN_14b0_0122 / 0251 / 0060, with their arguments), the random numbers
drawn and every FUN_14b0_0bbf blit it makes (12 argument words); on return
AL/AX, the widget list and the DGROUP globals it sets.
Writes tests/vectors/room_<NAME>.json.

    python tests/oracle/room_vectors.py NAME SNAPSHOT TICKS [A B]
    python tests/oracle/room_vectors.py NAME SNAPSHOT 0 --direct LOC:MODE... [HEXADDR=HEXBYTE...]

The second form builds the given scenes directly (see direct()).
"""
from __future__ import annotations

import json
import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402
from duel_watch import DS, lin, setup_duel  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def widgets(m: DosMachine) -> list[list]:
    out = []
    for i in range(m.read(DS, 0x969D, 1)[0]):
        ps, po = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
        ws, wo = m.u16(ps, po + 2), m.u16(ps, po)
        out.append([(ps << 16) | po, m.read(ws, wo, 0x36).hex()])
    return out


def direct(m: DosMachine, targets: list[tuple[int, int]], name: str, built,
           pokes: dict[int, int]) -> None:
    """At successive entries of the scene frame FUN_18cb_0002, call
    FUN_2b79_0ea5(loc, mode) and, when it finds the record,
    FUN_2b79_0003(1) (build and present), then let the scene run 40 ticks
    and save a screenshot room_NAME_LOC_MODE.png. `pokes` (DGROUP offset ->
    byte) are written before each build, to reach the builder's other
    paths (scene mode, field type, objects lying about, army flags)."""
    frame = lin(0x18CB, 0x0002)
    state = {"i": 0, "phase": "idle", "sp": 0, "wait": 0}
    ret_cs, ret_ip = 0x18CB - 0x1000 + LOAD_SEG, 0x0002

    def call(seg: int, off: int, args: list[int]) -> None:
        for a in reversed(args):
            m.push(a)
        m.push(ret_cs)
        m.push(ret_ip)
        m.w("cs", seg - 0x1000 + LOAD_SEG)
        m.w("ip", off)

    def hook(uc, addr, size, user):
        st = state
        if st["phase"] == "idle":
            if st["wait"] > 0 or st["i"] >= len(targets):
                st["wait"] -= 1
                return
            loc, mode = targets[st["i"]]
            for a, v in pokes.items():
                m.write(DS, a, bytes([v]))
            st["sp"] = m.r("sp")
            st["phase"] = "choose"
            call(0x2B79, 0x0EA5, [loc, mode])
            uc.emu_stop()
        elif st["phase"] == "choose":
            ok = m.r("ax") & 0xFF
            m.w("sp", st["sp"])
            if ok:
                st["phase"] = "build"
                call(0x2B79, 0x0003, [1])
            else:
                print(f"no record for {targets[st['i']]}")
                st["phase"], st["i"] = "idle", st["i"] + 1
            uc.emu_stop()
        elif st["phase"] == "build":
            m.w("sp", st["sp"])
            st["phase"], st["wait"] = "shot", 40
            uc.emu_stop()

    m.uc.hook_add(UC_HOOK_CODE, hook, begin=frame, end=frame)
    guard = 0
    while state["i"] < len(targets) and guard < 200000:
        guard += 1
        m.run(1)
        if state["phase"] == "shot" and state["wait"] <= 0:
            loc, mode = targets[state["i"]]
            m.save_png(OUT / f"room_{name}_{loc}_{mode}.png")
            print(f"built {loc}:{mode} ({built()} recorded)", flush=True)
            state["phase"], state["i"], state["wait"] = "idle", state["i"] + 1, 2
        elif state["phase"] == "shot":
            state["wait"] -= 1


def main(argv: list[str]) -> int:
    name, snap, ticks = argv[0], argv[1], int(argv[2])
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    targets, pokes = [], {}
    if len(argv) > 3 and argv[3] == "--direct":
        for t in argv[4:]:
            if "=" in t:
                a, v = t.split("=")
                pokes[int(a, 16)] = int(v, 16)
            else:
                targets.append(tuple(int(v, 0) for v in t.split(":")))
    elif len(argv) > 4:
        setup_duel(m, int(argv[3]), int(argv[4]))
    runs: list[dict] = []
    cur: dict | None = None

    def start(uc, addr, size, user):
        nonlocal cur
        ss, sp = m.r("ss"), m.r("sp")
        rs, ro = m.u16(DS, 0x72E8), m.u16(DS, 0x72E6)
        hs, ho = m.u16(rs, ro + 2), m.u16(rs, ro)
        count = m.read(hs, ho, 1)[0]
        cur = {"arg": m.u16(ss, sp + 4), "ds": m.read(DS, 0, 0x10000).hex(),
               "record": m.read(hs, ho, 2 + 4 * count + 4).hex(), "widgets_in": widgets(m),
               "images": [], "rng": [], "blits": []}

    def image(kind: str, nargs: int):
        def hook(uc, addr, size, user):
            if cur is None:
                return
            ss, sp = m.r("ss"), m.r("sp")
            args = [m.u16(ss, sp + 2 * k) for k in range(nargs)]
            seg, off = m.r("dx"), m.r("ax")
            header = ""
            if seg or off:
                hs, ho = m.u16(seg, off + 2), m.u16(seg, off)
                header = m.read(hs, ho, 0x40).hex()
            cur["images"].append([kind, args, (seg << 16) | off, header])
        return hook

    def rng(uc, addr, size, user):
        if cur is not None:
            cur["rng"].append(m.r("ax"))

    def blit(uc, addr, size, user):
        if cur is not None:
            ss, sp = m.r("ss"), m.r("sp")
            site = addr - lin(0x2B79, 0)
            cur["blits"].append([site, [m.u16(ss, sp + 2 * k) for k in range(12)]])

    def finish(uc, addr, size, user):
        nonlocal cur
        if cur is None:
            return
        cur["ax"] = m.r("ax")
        cur["widgets_out"] = widgets(m)
        cur["ds_out"] = {f"{a:04x}": m.read(DS, a, n).hex() for a, n in
                         ((0x9D10, 4), (0x9D19, 1), (0x9DC1, 2), (0x8E5D, 4), (0x9BA0, 0x60), (0x99BF, 8))}
        runs.append(cur)
        cur = None

    hook = lambda f, seg, off: m.uc.hook_add(UC_HOOK_CODE, f, begin=lin(seg, off), end=lin(seg, off))
    hook(start, 0x2B79, 0x0003)
    hook(finish, 0x2B79, 0x0EA4)
    hook(image("0251", 2), 0x2B79, 0x020D)
    hook(image("0122", 1), 0x2B79, 0x023D)
    hook(image("0060", 2), 0x2B79, 0x071C)
    hook(image("0122", 1), 0x2B79, 0x0D33)
    hook(image("0122", 1), 0x15B8, 0x049A)
    hook(image("0251", 2), 0x15B8, 0x051A)
    hook(rng, 0x15B8, 0x182E)
    for site in (0x00C0, 0x03F0, 0x0757, 0x0C81, 0x0E3E):
        hook(blit, 0x2B79, site)
    if targets:
        direct(m, targets, name, lambda: len(runs), pokes)
    else:
        m.run(ticks)
    m.save_png(OUT / f"room_{name}.png")
    print(f"{len(runs)} builds: " + ", ".join(
        f"ax={r['ax']:04x} objs={r['record'][:2]} blits={len(r['blits'])} widgets={len(r['widgets_out'])}"
        for r in runs))
    path = ROOT / "tests" / "vectors" / f"room_{name}.json"
    path.write_text(json.dumps({"runs": runs}))
    print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
