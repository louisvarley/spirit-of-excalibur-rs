#!/usr/bin/env python3
"""Oracle vectors for FUN_14b0_0451, the game text formatter.

The formatter calls FUN_14b0_099b to draw each character.  We patch that entry
to a far return and capture the character argument at the call site, so the
oracle checks formatting/layout without a video driver.

The EXE needs its MZ relocations applied: Ghidra addresses are +0x1000 from the
raw image (the load segment), so a far call whose raw operand is 0x2298 lands
at 0x3298 at runtime.
"""
from __future__ import annotations

import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tests" / "oracle"))

from excal_oracle import Oracle, STACK_SEG  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import (  # noqa: E402
    UC_X86_REG_DS,
    UC_X86_REG_ES,
    UC_X86_REG_SP,
)

DGROUP = 0x3364
FORMATTER = (0x14B0, 0x0451)
OUTPUT = 0x14B0 * 16 + 0x099B
DGROUP_PAYLOAD = 0x23640


def _relocated_payload(exe: Path) -> bytes:
    data = bytearray(exe.read_bytes())
    header = struct.unpack_from("<H", data, 8)[0]
    payload = bytearray(data[header * 16 :])
    reloc_off = struct.unpack_from("<H", data, 24)[0]
    count = struct.unpack_from("<H", data, 6)[0]
    for index in range(count):
        off, seg = struct.unpack_from("<HH", data, reloc_off + index * 4)
        at = seg * 16 + off
        value = struct.unpack_from("<H", payload, at)[0]
        struct.pack_into("<H", payload, at, (value + 0x1000) & 0xFFFF)
    return bytes(payload)


def _machine():
    oracle = Oracle()
    payload = _relocated_payload(ROOT / "ghidra" / "EXCAL_UNPACKED.EXE")
    uc = oracle._machine()
    uc.mem_write(0x10000, payload)
    uc.mem_write(DGROUP * 16, payload[DGROUP_PAYLOAD : DGROUP_PAYLOAD + 0x81BB])
    uc.reg_write(UC_X86_REG_DS, DGROUP)
    uc.reg_write(UC_X86_REG_ES, DGROUP)
    uc.mem_write(OUTPUT, b"\xcb")  # RETF: capture and discard the character
    return oracle, uc


def run(format_string: bytes, items: list[tuple[str, int | bytes]]) -> str:
    oracle, uc = _machine()
    uc.mem_write(DGROUP * 16 + 0x3000, format_string + b"\0")
    blobs: list[bytes] = []
    string_index = 0
    for kind, value in items:
        if kind == "word":
            blobs.append(struct.pack("<H", value & 0xFFFF))
        elif kind == "dword":
            blobs.append(struct.pack("<I", value & 0xFFFFFFFF))
        elif kind == "str":
            off = 0x4100 + string_index * 0x80
            string_index += 1
            uc.mem_write(DGROUP * 16 + off, value + b"\0")
            blobs.append(struct.pack("<HH", off, DGROUP))
        else:
            raise ValueError(kind)
    uc.mem_write(DGROUP * 16 + 0x4000, b"".join(blobs))

    out: list[int] = []

    def hook(uc_, address, size, user):
        if address == OUTPUT:
            sp = uc_.reg_read(UC_X86_REG_SP)
            word = struct.unpack(
                "<H", uc_.mem_read((STACK_SEG << 4) + (sp & 0xFFFF) + 4, 2)
            )[0]
            out.append(word & 0xFF)

    uc.hook_add(UC_HOOK_CODE, hook)
    oracle._call_far(
        uc,
        *FORMATTER,
        [0, 0x9000, 0, 0, 0x3000, DGROUP, 0x4000, DGROUP],
    )
    return bytes(out).decode("latin1")


CASES = [
    (b"Hi %d %s", [("word", 42), ("str", b"world")]),
    (b"%S sees %d %Ss", [("str", b"arthur"), ("word", 3), ("str", b"rat")]),
    (
        b"%d %u %l %c",
        [("word", 0xFFFB), ("word", 0xFFFF), ("dword", 100000), ("word", ord("Q"))],
    ),
    (b"%5d gold", [("word", 12)]),
    (b"%NsSighting %S", [("str", b"unused"), ("str", b"arthur")]),
    (b"%Wquote%V", [("word", 280)]),
    (b"100%% done", []),
    (b"a%Vb", []),
    (b"%N", [("word", 1), ("word", 2)]),
    (b"%s|%S", [("str", b"peace"), ("str", b"arthur")]),
]


def main() -> None:
    for format_string, items in CASES:
        result = run(format_string, items)
        print(
            json.dumps(
                {"format": format_string.decode("latin1"), "output": result},
                ensure_ascii=False,
            )
        )


if __name__ == "__main__":
    main()
