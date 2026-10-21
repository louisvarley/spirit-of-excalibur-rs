#!/usr/bin/env python3
"""Record golden vectors for the combat step FUN_20b5_1dff.

Runs a battle in the original from an emulator snapshot (by default the duel
from duel_watch.py), choosing "Automatic", and records every call of
1dff: the opcode word, the acting and target widgets, the character records,
the battle slot bytes at DS:9C82, the globals it reads, the random numbers
it draws (FUN_15b8_17e0), the message formats it shows (FUN_20b5_0000) and
the state after the call. Writes tests/vectors/battle_step.json.

    python tests/oracle/battle_vectors.py [SNAPSHOT] [TICKS] [RUNS]

Each run waits a different number of ticks first; the random generator mixes
in the timer, so every run plays out differently. Calls into the routines
1dff delegates to (unit removal, morale, stat changes, disposition) are
recorded with their arguments as "subcalls".
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
GLOBALS = [(0x01F2, 1), (0x032D, 1), (0x5E37, 1), (0x5E41, 4), (0x7D3E, 4), (0x7E3E, 4), (0x9A6C, 2),
           (0x9C4E, 0x0A), (0x9C56, 2), (0x9C82, 0x28), (0x9DAC, 2), (0x9DAE, 3), (0x9DC3, 0x32)]


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


SUBCALLS = {
    (0x20B5, 0x0140): ("remove_unit", 2), (0x20B5, 0x1705): ("rout", 3),
    (0x20B5, 0x1057): ("dead_unit", 1), (0x1BA8, 0x2611): ("stat_change", 3),
    (0x26C5, 0x041B): ("disposition", 3), (0x15B8, 0x11AC): ("mark", 3),
}


def main(argv: list[str]) -> int:
    snap = argv[0] if argv else "duel2"
    ticks = int(argv[1]) if len(argv) > 1 else 1500
    runs = int(argv[2]) if len(argv) > 2 else 1
    all_calls = []
    dgroup = None
    for run in range(runs):
        calls, ds = record(snap, ticks, run * 7)
        all_calls += calls
        dgroup = dgroup or ds
    path = ROOT / "tests" / "vectors" / "battle_step.json"
    path.write_text(json.dumps({"dgroup": dgroup, "calls": all_calls}))
    ops = {}
    for c in all_calls:
        ops[c["op"] >> 8] = ops.get(c["op"] >> 8, 0) + 1
    print(f"{len(all_calls)} calls, opcodes {dict(sorted(ops.items()))}; wrote {path}")
    return 0


def record(snap: str, ticks: int, delay: int) -> list[dict]:
    m = DosMachine()
    m.load("EXCAL.EXE")
    m.restore(pickle.loads((OUT / f"{snap}.snap").read_bytes()))
    dgroup = m.read(DS, 0, 0x10000).hex()

    def widgets() -> list[tuple[int, int]]:
        out = []
        for i in range(m.read(DS, 0x969D, 1)[0]):
            seg, off = m.u16(DS, 0x969F + 4 * i + 2), m.u16(DS, 0x969F + 4 * i)
            out.append((m.u16(seg, off + 2), m.u16(seg, off)))
        return out

    def find(cid: int):
        """FUN_15b8_16ca(0x200, 0, id): first widget with this id and flag 0x200."""
        for seg, off in widgets():
            if m.u16(seg, off + 0x0A) == cid and m.u16(seg, off) & 0x200:
                return seg, off
        return None

    def state(actor, target) -> dict:
        return {
            "chars": m.read(DS, 0x2C5E, 0x5D * 0x38 + 0x38).hex(),
            "globals": {f"{a:04x}": m.read(DS, a, n).hex() for a, n in GLOBALS},
            "actor": m.read(*actor, WIDGET).hex() if actor else None,
            "target": m.read(*target, WIDGET).hex() if target else None,
        }

    calls: list[dict] = []
    cur: dict = {}

    def entry(uc, addr, size, user):
        ss, sp = m.r("ss"), m.r("sp")
        op = m.u16(ss, sp + 4)
        aseg, aoff = m.u16(DS, 0x9A70), m.u16(DS, 0x9A6E)
        tid = m.read(aseg, aoff + 0x2E, 1)[0]
        target = find(tid)
        cur.clear()
        # [BP-0x10] is read uninitialised by opcode 6: the word that will sit
        # there once the frame is built (BP = SP-2 after PUSH BP).
        stale = m.u16(ss, (sp - 2 - 0x10) & 0xFFFF)
        cur.update(op=op, actor_ptr=(aseg, aoff), target_ptr=target, target_id=tid,
                   stale_bp10=stale, rout_ret=[],
                   before=state((aseg, aoff), target), rng=[], messages=[], subcalls=[])

    def rng(uc, addr, size, user):
        if cur:
            ss, sp = m.r("ss"), m.r("sp")
            ip, cs = m.u16(ss, sp), m.u16(ss, sp + 2)
            # Only draws made by 1dff itself (not by routines it calls).
            if cs == 0x20B5 - 0x1000 + LOAD_SEG and 0x1DFF <= ip <= 0x2A2F:
                cur["rng"].append(m.r("ax"))

    def message(uc, addr, size, user):
        if cur:
            ss, sp = m.r("ss"), m.r("sp")
            # PUSH CS; CALL near: IP, CS, then the format offset.
            cur["messages"].append(m.u16(ss, sp + 4))

    def exit_(uc, addr, size, user):
        if not cur:
            return
        rec = dict(cur)
        rec["ret"] = m.r("ax")
        rec["after"] = state(rec.pop("actor_ptr"), rec.pop("target_ptr"))
        calls.append(rec)
        cur.clear()

    def sub(name, nargs):
        def h(uc, addr, size, user):
            if cur:
                ss, sp = m.r("ss"), m.r("sp")
                # Same-segment callees are reached with PUSH CS; CALL near,
                # so every call here has a far return address.
                ret_ip = m.u16(ss, sp)
                ret_cs = m.u16(ss, sp + 2)
                # Only calls made by 1dff itself, not nested ones.
                if ret_cs != 0x20B5 - 0x1000 + LOAD_SEG or not 0x1DFF <= ret_ip <= 0x2A2F:
                    return
                base = sp + 4
                cur["subcalls"].append([name] + [m.u16(ss, base + 2 * k) for k in range(nargs)])
        return h

    for (seg, off), (name, nargs) in SUBCALLS.items():
        m.uc.hook_add(UC_HOOK_CODE, sub(name, nargs), begin=lin(seg, off), end=lin(seg, off))

    def rout_ret(uc, addr, size, user):
        if cur:
            cur["rout_ret"].append(m.r("ax") & 0xFF)

    m.uc.hook_add(UC_HOOK_CODE, rout_ret, begin=lin(0x20B5, 0x2139), end=lin(0x20B5, 0x2139))

    for addr, fn in ((lin(0x20B5, 0x1DFF), entry), (lin(0x15B8, 0x182E), rng),
                     (lin(0x20B5, 0x0000), message), (lin(0x20B5, 0x2A2F), exit_)):
        m.uc.hook_add(UC_HOOK_CODE, fn, begin=addr, end=addr)

    # The duel starts in automatic mode; clicking "Automatic" would turn it off.
    m.run(delay)
    for i in range(ticks // 100):
        m.run(100)
    print(f"delay {delay}: {len(calls)} calls", flush=True)
    return calls, dgroup


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
