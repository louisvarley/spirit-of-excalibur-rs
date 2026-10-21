#!/usr/bin/env python3
"""Record golden vectors for the army battle in segment 2449.

Starts army battles in the original from a map snapshot (see
army_watch.py) and records, for every call of

    FUN_2449_0aed  battle tokens            FUN_2449_0255  targeting
    FUN_2449_0faa  strength totals          FUN_2449_1021  rout check
    FUN_2449_12de  script handler 15
    2449:1653      one round of FUN_2449_134e (to 15cb or 26d8)

the DGROUP regions the battle owns and every widget before and after, the
random numbers drawn (FUN_15b8_17e0), the return value, and the calls into
routines that are not ported with them (messages with their arguments
resolved, the dead marks, unit-row redraws, music, map moves, message
boxes, the fall-back question). Those calls, and the UI waits
FUN_15b8_09f7, are made to return at once while a call is being recorded,
so the recorded state is that function's own work. For a round, the
134e locals (BP-0x84..BP) are recorded too.

Writes tests/vectors/army_calls.json.

    python tests/oracle/army_vectors.py [SNAPSHOT]
"""
from __future__ import annotations

import json
import pickle
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from army_watch import start_army_battle  # noqa: E402
from dosemu import DosMachine  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DS = 0x3364 - 0x1000 + LOAD_SEG
WIDGET = 0x36
REGIONS = [(0x01E0, 0x200), (0x2C5E, 0x5E * 0x38), (0x4FBF, 4), (0x5E30, 0x10), (0x64B0, 0xA8),
           (0x7E3E, 0x0C), (0x9C3E, 0x1D0)]
# Recorded functions: entry offset -> (name, return offsets, argument words).
FUNCS = {0x0AED: ("0aed", [0x0FA9], 0), 0x0255: ("0255", [0x040F], 0), 0x0FAA: ("0faa", [0x1020], 0),
         0x1021: ("1021", [0x12DD], 4), 0x12DE: ("12de", [0x1332], 1)}
ROUND = 0x1653
ROUND_END = (0x15CB, 0x26D8)
# Calls stubbed (return at once) while recording: (seg, off) -> (name, words).
STUBS = {
    (0x20B5, 0x0000): ("message", None), (0x15B8, 0x11AC): ("mark", 3), (0x20B5, 0x0140): ("unit_row", 2),
    (0x14B0, 0x0BBF): ("blit", 0), (0x14B0, 0x03F5): ("text", 0), (0x17F3, 0x003A): ("mouse_off", 0),
    (0x17F3, 0x0086): ("mouse_on", 0), (0x1316, 0x187B): ("music", 4), (0x1B06, 0x071C): ("map_071c", 2),
    (0x1772, 0x000E): ("box", 0),
}
# Near calls in 2449 (PUSH CS; CALL): stubbed the same way.
NEAR_STUBS = {0x0053: ("notice", 2), 0x0107: ("fall_back", 0)}
LIMIT = {"0255": 400, "12de": 400}


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


class Recorder:
    def __init__(self, m: DosMachine, label: str):
        self.m = m
        self.label = label
        self.active: list[dict] = []
        self.calls: list[dict] = []
        self.counts: dict[str, int] = {}
        self.fall_back = 1
        self.pokes = None

    # -- state -------------------------------------------------------
    def widgets(self) -> list[str]:
        m = self.m
        out = []
        for i in range(m.read(DS, 0x969D, 1)[0]):
            s, o = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
            out.append(m.read(m.u16(s, o + 2), m.u16(s, o), WIDGET).hex())
        return out

    def state(self) -> dict:
        m = self.m
        return {"regions": {f"{a:04x}": m.read(DS, a, n).hex() for a, n in REGIONS},
                "widgets": self.widgets(), "current": m.u16(DS, 0x9A6C)}

    def ret_now(self, ax: int | None = None) -> None:
        m = self.m
        ss, sp = m.r("ss"), m.r("sp")
        m.w("ip", m.u16(ss, sp))
        m.w("cs", m.u16(ss, sp + 2))
        m.w("sp", sp + 4)
        if ax is not None:
            m.w("ax", ax)

    def string(self, off: int, seg: int) -> str:
        return self.m.asciiz(seg, off, 80) if (seg or off) else ""

    def message_args(self, sp: int) -> dict:
        """FUN_20b5_0000(format far, ...): resolve the arguments by the
        format's conversions."""
        m = self.m
        ss = m.r("ss")
        fo, fs = m.u16(ss, sp + 4), m.u16(ss, sp + 6)
        fmt = m.asciiz(fs, fo, 120)
        at = sp + 8
        args = []
        i = 0
        while i < len(fmt):
            if fmt[i] == "%" and i + 1 < len(fmt):
                c = fmt[i + 1]
                i += 2
                if c in "sS":
                    args.append(self.string(m.u16(ss, at), m.u16(ss, at + 2)))
                    at += 4
                elif c == "l":
                    args.append(m.u16(ss, at) | m.u16(ss, at + 2) << 16)
                    at += 4
                elif c in "dcuxW":
                    args.append(m.u16(ss, at))
                    at += 2
                elif c == "%":
                    pass
                else:
                    args.append(f"?{c}")
                continue
            i += 1
        return {"format": fo if fs == DS else (fs << 16 | fo), "text": fmt, "args": args}

    # -- hooks -------------------------------------------------------
    def install(self) -> None:
        m = self.m
        hook = lambda f, seg, off: m.uc.hook_add(UC_HOOK_CODE, f, begin=lin(seg, off), end=lin(seg, off))
        for off, (name, rets, nargs) in FUNCS.items():
            hook(self.entry(name, nargs), 0x2449, off)
            for r in rets:
                hook(self.leave(name), 0x2449, r)
        hook(self.round_start, 0x2449, ROUND)
        for off in ROUND_END:
            hook(self.round_end, 0x2449, off)
        hook(self.rng, 0x15B8, 0x182E)
        hook(self.wait, 0x15B8, 0x09F7)
        for (seg, off), (name, n) in STUBS.items():
            hook(self.stub(name, n), seg, off)
        for off, (name, n) in NEAR_STUBS.items():
            hook(self.stub(name, n), 0x2449, off)
        hook(self.before_targeting, 0x2449, 0x15D6)

    def recording(self) -> bool:
        return bool(self.active)

    def entry(self, name: str, nargs: int):
        def h(uc, addr, size, user):
            if self.counts.get(name, 0) >= LIMIT.get(name, 10**9):
                return
            m = self.m
            ss, sp = m.r("ss"), m.r("sp")
            self.counts[name] = self.counts.get(name, 0) + 1
            self.active.append({"fn": name, "run": self.label, "args": [m.u16(ss, sp + 4 + 2 * k) for k in range(nargs)],
                                "rng": [], "subcalls": [], "before": self.state(), "depth": len(self.active)})
        return h

    def leave(self, name: str):
        def h(uc, addr, size, user):
            for k in range(len(self.active) - 1, -1, -1):
                if self.active[k]["fn"] == name:
                    call = self.active.pop(k)
                    call["ret"] = self.m.r("ax")
                    call["after"] = self.state()
                    self.calls.append(call)
                    break
        return h

    def round_start(self, uc, addr, size, user):
        m = self.m
        bp = m.r("bp")
        ss = m.r("ss")
        self.active.append({"fn": "round", "run": self.label, "args": [], "rng": [], "subcalls": [],
                            "locals_before": m.read(ss, bp - 0x84, 0x84).hex(), "before": self.state(),
                            "depth": len(self.active)})

    def round_end(self, uc, addr, size, user):
        for k in range(len(self.active) - 1, -1, -1):
            if self.active[k]["fn"] == "round":
                call = self.active.pop(k)
                m = self.m
                call["end"] = m.r("ip")
                call["locals_after"] = m.read(m.r("ss"), m.r("bp") - 0x84, 0x84).hex()
                call["after"] = self.state()
                self.calls.append(call)
                break

    def rng(self, uc, addr, size, user):
        for c in self.active:
            c["rng"].append(self.m.r("ax"))

    def wait(self, uc, addr, size, user):
        if self.recording():
            m = self.m
            ss, sp = m.r("ss"), m.r("sp")
            flag = m.u16(ss, sp + 4)
            for c in self.active:
                c["subcalls"].append(["wait", flag])
            self.ret_now()

    def stub(self, name: str, n):
        def h(uc, addr, size, user):
            if not self.recording():
                return
            m = self.m
            ss, sp = m.r("ss"), m.r("sp")
            if name == "message":
                rec = ["message", self.message_args(sp)]
            else:
                rec = [name] + [m.u16(ss, sp + 4 + 2 * k) for k in range(n or 0)]
            if name == "notice":
                rec = ["notice", m.u16(ss, sp + 4)]
            ax = None
            if name == "fall_back":
                ax = self.fall_back
                rec.append(ax)
            for c in self.active:
                c["subcalls"].append(rec)
            self.ret_now(ax)
        return h

    def before_targeting(self, uc, addr, size, user):
        """Before each pass of the order loop, optionally give the player's
        units orders the way the order buttons do (slot states 2 charge,
        3 flank, 6 retreat, 1 engage), to cover the round's order paths."""
        if self.pokes is None:
            return
        m = self.m
        n = m.read(DS, 0x9DAC, 1)[0]
        for i in range(n):
            if self.pokes.random() < 0.08:
                s = m.read(DS, 0x9C82 + i, 1)[0]
                if s in (1,):
                    m.write(DS, 0x9C82 + i, bytes([self.pokes.choice([2, 3, 1, 1])]))


def run(snap: str, side1: list[int], side2: list[int], mode: int, delay: int, ticks: int,
        pokes: int | None, label: str, fall_back: int = 1) -> tuple[list[dict], str]:
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    m.run(delay)
    rec = Recorder(m, label)
    rec.fall_back = fall_back
    rec.pokes = random.Random(pokes) if pokes is not None else None
    base = None
    st = start_army_battle(m, side1, side2, mode)
    rec.install()
    for t in range(ticks):
        m.run(1)
        if base is None and rec.calls:
            pass
        if st["done"]:
            break
    base = m.read(DS, 0, 0x10000).hex()
    rounds = sum(1 for c in rec.calls if c["fn"] == "round")
    print(f"{label}: {len(rec.calls)} calls, {rounds} rounds, done={st['done']} t={m.ticks}", flush=True)
    return rec.calls, base


def main(argv: list[str]) -> int:
    snap = argv[0] if argv else "map"
    runs = [
        # (side1, side2, mode, delay, ticks, pokes, label, fall_back)
        ([1, 6, 50], [56, 57, 58], 0x10, 0, 6000, None, "a", 1),
        ([1, 6, 50], [56, 57, 58], 0x30, 3, 3000, None, "b", 1),
        ([1, 6, 50, 59, 60, 61], [56, 57, 58], 0x10, 7, 6000, 1, "c", 1),
        ([1, 6, 50], [56, 57, 58, 59, 60, 61], 0x10, 11, 6000, 2, "d", 0),
        ([59, 60, 61], [56, 57, 58], 0x30, 5, 3000, None, "e", 1),
    ]
    calls = []
    base = None
    for r in runs:
        c, b = run(snap, *r)
        calls += c
        base = base or b
    path = ROOT / "tests" / "vectors" / "army_calls.json"
    path.write_text(json.dumps({"dgroup": base, "calls": calls}))
    by = {}
    for c in calls:
        by[c["fn"]] = by.get(c["fn"], 0) + 1
    print(f"wrote {path}: {by}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
