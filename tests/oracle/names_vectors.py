#!/usr/bin/env python3
"""Record golden vectors for the name builders.

Calls the original FUN_2d65_08a4(id) (display name: "the townsman",
"Sir Kay", "several soldiers") and FUN_15b8_16fe(name, count) (article:
"", "a ", "an ", "several ") for every character from an emulator snapshot,
and saves the DGROUP image they read. Writes tests/vectors/names.json.

    python tests/oracle/names_vectors.py SNAPSHOT
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


def main(argv: list[str]) -> int:
    m = DosMachine()
    m.load("EXCAL.EXE")
    base = pickle.loads((OUT / f"{argv[0]}.snap").read_bytes())
    m.restore(base)
    done = {"hit": False}

    def stop(uc, addr, size, user):
        done["hit"] = True
        uc.emu_stop()

    sent = (SENTINEL[0] << 4) + SENTINEL[1]
    m.uc.hook_add(UC_HOOK_CODE, stop, begin=sent, end=sent)

    def call(seg: int, off: int, args: list[int]) -> str:
        m.restore(base)
        done["hit"] = False
        for v in reversed(args):
            m.push(v)
        m.push(SENTINEL[0])
        m.push(SENTINEL[1])
        m.w("cs", seg - 0x1000 + LOAD_SEG)
        m.w("ip", off)
        for _ in range(20):
            m.run(1)
            if done["hit"]:
                return m.asciiz(m.r("dx"), m.r("ax"))
        raise RuntimeError(f"{seg:04x}:{off:04x}{args} did not return")

    names = {}
    for cid in range(1, 93):
        name_ptr = 0x2C5E + cid * 0x38
        count = m.u16(DS, name_ptr + 0x1E)
        names[str(cid)] = {
            "display": call(0x2D65, 0x08A4, [cid]),
            "article": call(0x15B8, 0x16FE, [name_ptr, DS, count]),
        }
    out = {"dgroup": m.read(DS, 0, 0x10000).hex(), "names": names}
    path = ROOT / "tests" / "vectors" / "names.json"
    path.write_text(json.dumps(out))
    sample = [(k, v["article"] + "|" + v["display"]) for k, v in list(names.items())[:12]]
    print(sample)
    print("wrote", path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
