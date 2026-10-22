#!/usr/bin/env python3
"""Record golden frames for the start-up intro, FUN_2c74_0006(1).

Boots the original and runs it until the intro returns. Every entry into
the intro's wait FUN_2c74_0b31 is a point where a frame has been fully
composed, so there (and after each palette-cycle step inside a wait) the recorder takes the mode 13h screen, the 6-bit DAC,
the game tick counter DS:7E3E and the wait argument. Consecutive identical
frames are stored once. Writes tests/vectors/intro/entry.json (clock,
DS:9D1C and DAC on entry) and tests/vectors/intro/frames.bin:

    repeated: u32 tick, u16 wait_arg, u16 count, 64000 pixels, 768 dac

`count` is how many waits in a row showed this frame. Also saves the
snapshot `intro_end` (in tests/vectors/emu/) taken when the intro returns.

    python tests/oracle/intro_vectors.py
"""
from __future__ import annotations

import json
import pickle
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine, VGA_LINEAR  # noqa: E402
from play import LOAD_SEG, OUT  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DS = 0x3364 - 0x1000 + LOAD_SEG
DEST = ROOT / "tests" / "vectors" / "intro"


def lin(seg: int, off: int) -> int:
    return ((seg - 0x1000 + LOAD_SEG) << 4) + off


def main() -> int:
    DEST.mkdir(parents=True, exist_ok=True)
    m = DosMachine()
    m.load("EXCAL.EXE")
    frames: list[list] = []
    state = {"entered": False, "done": False}

    def word(addr: int) -> int:
        return struct.unpack("<H", bytes(m.uc.mem_read(addr, 2)))[0]

    def on_wait(uc, addr, size, user):
        if not state["entered"]:
            return
        sp = (m.r("ss") << 4) + m.r("sp")
        arg = word(sp + 4)
        tick = word(DS * 16 + 0x7E3E) | word(DS * 16 + 0x7E40) << 16
        pixels = bytes(uc.mem_read(VGA_LINEAR, 64000))
        dac = bytes(m.dac[:768])
        if frames and frames[-1][3] == pixels and frames[-1][4] == dac:
            frames[-1][2] += 1
            return
        frames.append([tick, arg, 1, pixels, dac])

    def on_enter(uc, addr, size, user):
        state["entered"] = True
        # Entry state the Rust replay starts from: clock, DS:9D1C, DAC.
        entry = {
            "clock": word(DS * 16 + 0x7E3E) | word(DS * 16 + 0x7E40) << 16,
            "cycle_at": word(DS * 16 + 0x9D1C) | word(DS * 16 + 0x9D1E) << 16,
            "dac": list(m.dac[:96]),
        }
        (DEST / "entry.json").write_text(json.dumps(entry))

    def on_return(uc, addr, size, user):
        state["done"] = True
        uc.emu_stop()

    m.uc.hook_add(UC_HOOK_CODE, on_wait, begin=lin(0x2C74, 0x0B31), end=lin(0x2C74, 0x0B31))
    # After a palette-cycle step inside the wait (2c74:0bdc), so cycling
    # during the long 0x1E0 hold is sampled too; recorded with arg 0xFFFF.
    def on_cycle(uc, addr, size, user):
        if not state["entered"]:
            return
        tick = word(DS * 16 + 0x7E3E) | word(DS * 16 + 0x7E40) << 16
        pixels = bytes(uc.mem_read(VGA_LINEAR, 64000))
        dac = bytes(m.dac[:768])
        if frames and frames[-1][3] == pixels and frames[-1][4] == dac:
            return
        frames.append([tick, 0xFFFF, 1, pixels, dac])

    m.uc.hook_add(UC_HOOK_CODE, on_cycle, begin=lin(0x2C74, 0x0BDC), end=lin(0x2C74, 0x0BDC))
    m.uc.hook_add(UC_HOOK_CODE, on_enter, begin=lin(0x2C74, 0x0006), end=lin(0x2C74, 0x0006))
    # Return address of the intro call at 2d65:2e3c.
    m.uc.hook_add(UC_HOOK_CODE, on_return, begin=lin(0x2D65, 0x2E41), end=lin(0x2D65, 0x2E41))
    while not state["done"]:
        m.run(50)
        print(f"emu tick {m.ticks}, frames {len(frames)}", flush=True)
    (OUT / "intro_end.snap").write_bytes(pickle.dumps(m.snapshot()))
    with open(DEST / "frames.bin", "wb") as f:
        for tick, arg, count, pixels, dac in frames:
            f.write(struct.pack("<IHH", tick, arg, count) + pixels + dac)
    print(f"{len(frames)} frames -> {DEST / 'frames.bin'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
