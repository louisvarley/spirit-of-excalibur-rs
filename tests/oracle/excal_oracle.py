#!/usr/bin/env python3
"""Minimal real-mode Unicorn oracle for EXCAL_UNPACKED.EXE routines.

Image convention: Ghidra loads the unpacked payload at 1000:0000, which is
linear 0x10000. We load the payload there so Ghidra addresses are runtime
addresses (1057:0000 == 0x10570).
"""
from __future__ import annotations

import os
import struct
from pathlib import Path

from unicorn import Uc, UC_ARCH_X86, UC_MODE_16, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_AX, UC_X86_REG_DX, UC_X86_REG_CS, UC_X86_REG_DS, UC_X86_REG_ES, UC_X86_REG_SS, UC_X86_REG_SP,
    UC_X86_REG_IP,
)

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_EXE = ROOT / "ghidra" / "EXCAL_UNPACKED.EXE"
LOAD_SEG = 0x1000
SENTINEL_SEG = 0x9000
SENTINEL = 0x0000
STACK_SEG = 0x8000
SRC_SEG = 0x2000
DST_SEG = 0x3000
MEM_SIZE = 0x120000


def _load_payload(path: Path) -> bytes:
    data = path.read_bytes()
    if data[:2] != b"MZ":
        raise SystemExit(f"{path} is not an MZ executable")
    header_paras = struct.unpack_from("<H", data, 8)[0]
    return data[header_paras * 16:]


class Oracle:
    def __init__(self, exe: Path | None = None):
        exe = Path(os.environ.get("SOE_EXE", exe or DEFAULT_EXE))
        self.payload = _load_payload(exe)

    def _machine(self):
        uc = Uc(UC_ARCH_X86, UC_MODE_16)
        uc.mem_map(0, MEM_SIZE)
        uc.mem_write(LOAD_SEG * 16, self.payload)
        uc.reg_write(UC_X86_REG_CS, LOAD_SEG)
        uc.reg_write(UC_X86_REG_DS, LOAD_SEG)
        uc.reg_write(UC_X86_REG_ES, LOAD_SEG)
        uc.reg_write(UC_X86_REG_SS, STACK_SEG)
        uc.reg_write(UC_X86_REG_SP, 0xFFFE)
        self._stop = False

        def hook(uc_, address, size, user):
            self.last = address
            if address == (SENTINEL_SEG << 4) + SENTINEL:
                self._stop = True
                uc_.emu_stop()

        uc.hook_add(UC_HOOK_CODE, hook)
        return uc

    @staticmethod
    def _push16(uc, value: int):
        sp = uc.reg_read(UC_X86_REG_SP) - 2
        uc.reg_write(UC_X86_REG_SP, sp)
        uc.mem_write((STACK_SEG << 4) + (sp & 0xFFFF), struct.pack("<H", value & 0xFFFF))

    def _call_far(self, uc, seg: int, off: int, args: list[int]):
        # cdecl: push arguments first, then the far return address on top.
        for value in reversed(args):
            self._push16(uc, value)
        self._push16(uc, SENTINEL_SEG)
        self._push16(uc, SENTINEL)
        uc.reg_write(UC_X86_REG_CS, seg)
        uc.reg_write(UC_X86_REG_IP, off)
        try:
            uc.emu_start((seg << 4) + off, (SENTINEL_SEG << 4) + SENTINEL, count=50_000_000)
        except Exception as exc:
            cs = uc.reg_read(UC_X86_REG_CS); ip = uc.reg_read(UC_X86_REG_IP)
            raise RuntimeError(f"oracle stopped at {cs:04x}:{ip:04x} last={getattr(self,'last',0):x}: {exc}") from exc

    def lzss(self, src: bytes, expanded_size: int) -> bytes:
        uc = self._machine()
        uc.mem_write(SRC_SEG * 16, src)
        # args right-to-left: out_size, in_size, dest_seg, dest_off, src_seg, src_off
        self._call_far(uc, 0x1057, 0x0000,
                       [0, SRC_SEG, 0, DST_SEG, len(src), expanded_size])
        return bytes(uc.mem_read(DST_SEG * 16, expanded_size))

    def rle(self, src: bytes, max_output: int = 0x10000) -> bytes:
        uc = self._machine()
        # RLE requires source/destination segments to match and output high word 0.
        # The routine requires destination before source in the same segment.
        src_off = 0x8000
        uc.mem_write((SRC_SEG << 4) + src_off, src)
        self._call_far(uc, 0x30D5, 0x0110,
                       [src_off, SRC_SEG, 0, SRC_SEG, len(src), 0])
        count = uc.reg_read(UC_X86_REG_AX) & 0xFFFF
        return bytes(uc.mem_read(SRC_SEG << 4, min(count, max_output)))


if __name__ == "__main__":
    o = Oracle()
    print("loaded", len(o.payload), "bytes")
    # quick self-check: MUSIC first record should start FORM
    d = (ROOT / "game" / "MUSIC.RES").read_bytes()
    _, io, _, _ = struct.unpack_from("<IIII", d, 0)
    first = d[0x18:0x18 + 709]
    out = o.lzss(first, 1216)
    print("lzss head", out[:4])
    r = o.rle(bytes([2, 65, 66, 67, 0xFE, 90]), 16)
    print("rle", r)
