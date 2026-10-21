#!/usr/bin/env python3
"""Drive the original game in the headless DOS machine.

Usage:
    python tests/oracle/play.py [--from NAME] COMMAND...

Commands run left to right:
    boot              load EXCAL.EXE and run to the opening menu
    run N             run N timer ticks (18.2 per second of game time)
    key NAME          press and release a key (enter, esc, space, up, a, 1, f1...)
    type TEXT         type text (letters, digits, spaces)
    click X Y [HOLD]  left-click at logical (X, Y), button held HOLD ticks (default 40)
    rclick X Y        right-click
    move X Y          move the mouse
    shot NAME         save tests/vectors/emu/NAME.png
    save NAME         save snapshot tests/vectors/emu/NAME.snap
    state             print ticks, CS:IP (runtime and Ghidra) and video mode

Snapshots and screenshots contain the original game's memory and images, so
they live in the git-ignored tests/vectors/emu/.

Example:
    python tests/oracle/play.py boot save menu
    python tests/oracle/play.py --from menu click 90 29 run 300 shot camelot save camelot
"""
from __future__ import annotations

import pickle
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from dosemu import DosMachine  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "tests" / "vectors" / "emu"

# Runtime segment the unpacker places the program at; Ghidra uses 1000.
LOAD_SEG = 0x214


def ghidra(seg: int, off: int) -> str:
    return f"{seg - LOAD_SEG + 0x1000:04x}:{off:04x}"


def boot(m: DosMachine) -> None:
    m.load("EXCAL.EXE")
    m.run(400)          # loaders and the lake intro
    m.press("space")    # skip the intro
    m.run(300)          # menu fades in


def main(argv: list[str]) -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    log = (lambda s: print("  " + s)) if "-v" in argv else None
    argv = [a for a in argv if a != "-v"]
    m = DosMachine(log=log)
    if argv[:1] == ["--from"]:
        m.load("EXCAL.EXE")
        m.restore(pickle.loads((OUT / f"{argv[1]}.snap").read_bytes()))
        argv = argv[2:]
    args = list(argv)
    while args:
        cmd = args.pop(0)
        if cmd == "boot":
            boot(m)
        elif cmd == "run":
            m.run(int(args.pop(0)))
        elif cmd == "key":
            m.press(args.pop(0))
            m.run(10)
        elif cmd == "type":
            m.type(args.pop(0))
            m.run(10)
        elif cmd in ("click", "rclick"):
            x, y = int(args.pop(0)), int(args.pop(0))
            hold = int(args.pop(0)) if args and args[0].isdigit() else 40
            m.click(x, y, hold=hold, button=1 if cmd == "click" else 2)
        elif cmd == "move":
            m.move(int(args.pop(0)), int(args.pop(0)))
            m.run(4)
        elif cmd == "shot":
            path = OUT / f"{args.pop(0)}.png"
            m.save_png(path)
            print("shot", path)
        elif cmd == "save":
            path = OUT / f"{args.pop(0)}.snap"
            path.write_bytes(pickle.dumps(m.snapshot()))
            print("saved", path)
        elif cmd == "state":
            cs, ip = m.r("cs"), m.r("ip")
            print(f"ticks={m.ticks} cs:ip={cs:04x}:{ip:04x} ({ghidra(cs, ip)}) mode={m.video_mode:02x}")
        else:
            raise SystemExit(f"unknown command {cmd!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
