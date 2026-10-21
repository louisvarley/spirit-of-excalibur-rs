#!/usr/bin/env python3
"""Headless DOS machine for running the original game under Unicorn.

Just enough of a PC and of DOS to run the user's own EXCAL.EXE from `game/`:
real-mode interrupt dispatch, an MZ loader with PSP and environment, DOS
memory and file services mapped onto `game/`, the BIOS timer/keyboard/video
services, VGA mode 13h memory and DAC ports, and a deterministic timer that
fires IRQ0 every `insns_per_tick` instructions.

Nothing here is game-specific; scripts drive it through `DosMachine`.
"""
from __future__ import annotations

import os
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

from unicorn import (
    Uc, UcError, UC_ARCH_X86, UC_MODE_16, UC_HOOK_CODE, UC_HOOK_INTR, UC_HOOK_INSN,
)
from unicorn.x86_const import (
    UC_X86_REG_AX, UC_X86_REG_BX, UC_X86_REG_CX, UC_X86_REG_DX, UC_X86_REG_SI,
    UC_X86_REG_DI, UC_X86_REG_BP, UC_X86_REG_SP, UC_X86_REG_CS, UC_X86_REG_DS,
    UC_X86_REG_ES, UC_X86_REG_SS, UC_X86_REG_IP, UC_X86_REG_FLAGS,
    UC_X86_INS_IN, UC_X86_INS_OUT,
)

ROOT = Path(__file__).resolve().parents[2]
GAME_DIR = ROOT / "game"

MEM_SIZE = 0x110000
BIOS_SEG = 0xF000          # interrupt stubs live at F000:n*4
ENV_SEG = 0x0100
FIRST_FREE_SEG = 0x0200     # conventional memory handed out from here
TOP_SEG = 0xA000
VGA_LINEAR = 0xA0000

CF = 0x0001
ZF = 0x0040
IF = 0x0200

REGS16 = {
    "ax": UC_X86_REG_AX, "bx": UC_X86_REG_BX, "cx": UC_X86_REG_CX, "dx": UC_X86_REG_DX,
    "si": UC_X86_REG_SI, "di": UC_X86_REG_DI, "bp": UC_X86_REG_BP, "sp": UC_X86_REG_SP,
    "cs": UC_X86_REG_CS, "ds": UC_X86_REG_DS, "es": UC_X86_REG_ES, "ss": UC_X86_REG_SS,
    "ip": UC_X86_REG_IP, "flags": UC_X86_REG_FLAGS,
}

# Scan codes for scripted keys: name -> (scan, ascii).
KEYS = {
    "enter": (0x1C, 0x0D), "esc": (0x01, 0x1B), "space": (0x39, 0x20),
    "up": (0x48, 0), "down": (0x50, 0), "left": (0x4B, 0), "right": (0x4D, 0),
    "backspace": (0x0E, 0x08), "tab": (0x0F, 0x09),
    **{f"f{n}": (0x3A + n, 0) for n in range(1, 11)},
}
for _i, _c in enumerate("1234567890"):
    KEYS[_c] = (0x02 + _i, ord(_c))
for _row, _scan in (("qwertyuiop", 0x10), ("asdfghjkl", 0x1E), ("zxcvbnm", 0x2C)):
    for _i, _c in enumerate(_row):
        KEYS[_c] = (_scan + _i, ord(_c))


class Halt(Exception):
    """The program terminated (INT 21h/4Ch) or the machine stopped."""


@dataclass
class OpenFile:
    path: Path
    data: bytearray
    pos: int = 0
    dirty: bool = False


@dataclass
class Program:
    psp: int
    name: str


class DosMachine:
    def __init__(self, game_dir: Path = GAME_DIR, insns_per_tick: int = 40_000,
                 log=None, trace_ints: bool = False):
        self.game_dir = Path(game_dir)
        self.insns_per_tick = insns_per_tick
        self.log = log or (lambda *a: None)
        self.trace_ints = trace_ints
        self.uc = uc = Uc(UC_ARCH_X86, UC_MODE_16)
        uc.mem_map(0, MEM_SIZE)
        self.files: dict[int, OpenFile] = {}
        self.next_handle = 5
        self.dta = (0, 0x80)
        self.find_state: list[str] = []
        self.blocks: dict[int, int] = {}       # seg -> paragraphs
        self.programs: list[Program] = []
        self.exit_code: int | None = None
        self.ticks = 0
        self.insns = 0
        self.dac = bytearray(768)
        self.dac_write = 0
        self.dac_read = 0
        self.dac_sub = 0
        self.video_mode = 3
        self.keyq: list[tuple[int, int]] = []   # (scan, ascii) for INT 16h
        self.scanq: list[int] = []              # raw make/break codes for port 60h
        self.last_scan = 0
        self.mouse = {"x": 160, "y": 100, "buttons": 0, "shown": 0}
        self.port_log: dict[int, int] = {}
        self.pit_reload = 0
        self.int_counts: dict[tuple[int, int], int] = {}
        self.stopped = False

        self._setup_bios()
        uc.hook_add(UC_HOOK_INTR, self._on_intr)
        uc.hook_add(UC_HOOK_CODE, self._on_stub, begin=BIOS_SEG << 4,
                    end=(BIOS_SEG << 4) + 0x400)
        uc.hook_add(UC_HOOK_INSN, self._on_in, None, 1, 0, UC_X86_INS_IN)
        uc.hook_add(UC_HOOK_INSN, self._on_out, None, 1, 0, UC_X86_INS_OUT)

    # ------------------------------------------------------------------ regs
    def r(self, name: str) -> int:
        return self.uc.reg_read(REGS16[name]) & 0xFFFF

    def w(self, name: str, value: int) -> None:
        self.uc.reg_write(REGS16[name], value & 0xFFFF)

    def rb(self, name: str) -> int:
        reg, half = name[0] + "x", name[1]
        v = self.r(reg)
        return (v >> 8) & 0xFF if half == "h" else v & 0xFF

    def wb(self, name: str, value: int) -> None:
        reg, half = name[0] + "x", name[1]
        v = self.r(reg)
        v = (v & 0x00FF) | ((value & 0xFF) << 8) if half == "h" else (v & 0xFF00) | (value & 0xFF)
        self.w(reg, v)

    def read(self, seg: int, off: int, n: int) -> bytes:
        return bytes(self.uc.mem_read(((seg << 4) + off) & 0xFFFFF, n))

    def write(self, seg: int, off: int, data: bytes) -> None:
        self.uc.mem_write(((seg << 4) + off) & 0xFFFFF, bytes(data))

    def u16(self, seg: int, off: int) -> int:
        return struct.unpack("<H", self.read(seg, off, 2))[0]

    def put16(self, seg: int, off: int, v: int) -> None:
        self.write(seg, off, struct.pack("<H", v & 0xFFFF))

    def asciiz(self, seg: int, off: int, limit: int = 128) -> str:
        raw = self.read(seg, off, limit)
        return raw.split(b"\0", 1)[0].decode("latin-1")

    def push(self, v: int) -> None:
        sp = (self.r("sp") - 2) & 0xFFFF
        self.w("sp", sp)
        self.put16(self.r("ss"), sp, v)

    def pop(self) -> int:
        sp = self.r("sp")
        v = self.u16(self.r("ss"), sp)
        self.w("sp", sp + 2)
        return v

    # ----------------------------------------------------------------- setup
    def _setup_bios(self) -> None:
        # IVT -> F000:n*4, each stub is a single IRET; _on_stub runs first.
        stubs = bytearray(b"\xCF" * 0x400)
        # Drivers are often detected by "vector does not point at IRET":
        # give the mouse a NOP first so INT 33h counts as installed.
        stubs[0x33 * 4] = 0x90
        self.write(BIOS_SEG, 0, stubs)
        for n in range(256):
            self.put16(0, n * 4, n * 4)
            self.put16(0, n * 4 + 2, BIOS_SEG)
        # BIOS data area.
        self.put16(0x40, 0x13, 640)          # base memory KiB
        self.write(0x40, 0x49, bytes([3]))    # video mode
        self.put16(0x40, 0x4A, 80)
        self.put16(0x40, 0x63, 0x3D4)
        self.write(0x40, 0x84, bytes([24]))
        self.put16(0x40, 0x1A, 0x1E)          # keyboard buffer head/tail
        self.put16(0x40, 0x1C, 0x1E)
        self.put16(0x40, 0x10, 0x0027)        # equipment: FPU absent, 80x25 colour

    def _alloc(self, paras: int) -> int | None:
        seg = FIRST_FREE_SEG
        for start in sorted(self.blocks):
            if start - seg >= paras:
                break
            seg = max(seg, start + self.blocks[start])
        if seg + paras > TOP_SEG:
            return None
        self.blocks[seg] = paras
        return seg

    def _largest_free(self) -> int:
        seg, best = FIRST_FREE_SEG, 0
        for start in sorted(self.blocks):
            best = max(best, start - seg)
            seg = max(seg, start + self.blocks[start])
        return max(best, TOP_SEG - seg)

    # ---------------------------------------------------------------- loader
    def load(self, name: str, tail: str = "") -> Program:
        """Load an MZ/COM program like INT 21h/4B00 and point CS:IP at it."""
        path = self.resolve(name)
        data = path.read_bytes()
        env = b"PATH=C:\\\0COMSPEC=C:\\COMMAND.COM\0\0\x01\0C:\\" + path.name.upper().encode() + b"\0"
        env_seg = self._alloc((len(env) + 15) // 16 + 1)
        self.write(env_seg, 0, env)
        if data[:2] in (b"MZ", b"ZM"):
            last, pages, nrel, hdr, minalloc, maxalloc, ss, sp, _chk, ip, cs, reloff = struct.unpack_from(
                "<HHHHHHHHHHHH", data, 2)
            image_len = pages * 512 - (512 - last if last else 0) - hdr * 16
            image = data[hdr * 16: hdr * 16 + image_len]
            need = (image_len + 15) // 16 + 0x10 + minalloc
            # Like DOS: give the program the largest block (maxalloc is FFFF).
            paras = min(self._largest_free(), max(need, (image_len + 15) // 16 + 0x10 + maxalloc))
            psp = self._alloc(paras)
            if psp is None:
                raise RuntimeError("not enough memory to load " + name)
            load_seg = psp + 0x10
            self.write(load_seg, 0, image)
            for i in range(nrel):
                off, seg = struct.unpack_from("<HH", data, reloff + 4 * i)
                v = self.u16(load_seg + seg, off)
                self.put16(load_seg + seg, off, v + load_seg)
            init = dict(cs=load_seg + cs, ip=ip, ss=load_seg + ss, sp=sp)
        else:
            paras = self._largest_free()
            psp = self._alloc(paras)
            self.write(psp, 0x100, data)
            init = dict(cs=psp, ip=0x100, ss=psp, sp=0xFFFE)
        # PSP.
        psp_bytes = bytearray(256)
        psp_bytes[0:2] = b"\xCD\x20"
        struct.pack_into("<H", psp_bytes, 2, psp + self.blocks[psp])
        struct.pack_into("<H", psp_bytes, 0x2C, env_seg)
        psp_bytes[0x18:0x2C] = bytes([0, 1, 2, 3, 4] + [0xFF] * 15)
        struct.pack_into("<HHH", psp_bytes, 0x32, 20, 0x18, psp)
        tail_b = (" " + tail if tail else "").encode()
        psp_bytes[0x80] = len(tail_b)
        psp_bytes[0x81:0x81 + len(tail_b)] = tail_b
        psp_bytes[0x81 + len(tail_b)] = 0x0D
        self.write(psp, 0, psp_bytes)
        prog = Program(psp=psp, name=path.name)
        self.programs.append(prog)
        self.dta = (psp, 0x80)
        for reg, v in init.items():
            self.w(reg, v)
        self.w("ds", psp)
        self.w("es", psp)
        self.w("ax", 0)
        self.w("flags", IF | 0x0002)
        self.log(f"load {path.name} psp={psp:04x} cs:ip={init['cs']:04x}:{init['ip']:04x}")
        return prog

    def resolve(self, name: str) -> Path:
        """Map a DOS path onto game_dir, case-insensitively."""
        name = name.replace("/", "\\")
        if len(name) > 1 and name[1] == ":":
            name = name[2:]
        parts = [p for p in name.split("\\") if p and p != "."]
        # Absolute paths such as \EXCAL\X are taken relative to game_dir.
        if name.startswith("\\") and parts and parts[0].upper() == "EXCAL":
            parts = parts[1:]
        cur = self.game_dir
        for part in parts:
            match = None
            if cur.is_dir():
                for entry in cur.iterdir():
                    if entry.name.upper() == part.upper():
                        match = entry
                        break
            cur = match if match else cur / part
        return cur

    # ----------------------------------------------------------- interrupts
    def _on_intr(self, uc, intno, user):
        # Software INT n: Unicorn leaves IP after the instruction; vector it
        # like a real-mode CPU through the IVT.
        self.raise_int(intno)

    def raise_int(self, n: int) -> None:
        self.push(self.r("flags"))
        self.push(self.r("cs"))
        self.push(self.r("ip"))
        self.w("flags", self.r("flags") & ~(IF | 0x0100))
        self.w("ip", self.u16(0, n * 4))
        self.w("cs", self.u16(0, n * 4 + 2))

    def _set_cf(self, on: bool) -> None:
        # Flags pushed by the INT are at SS:SP+4 when the stub runs.
        ss, sp = self.r("ss"), self.r("sp")
        f = self.u16(ss, sp + 4)
        self.put16(ss, sp + 4, (f | CF) if on else (f & ~CF))

    def _set_zf(self, on: bool) -> None:
        ss, sp = self.r("ss"), self.r("sp")
        f = self.u16(ss, sp + 4)
        self.put16(ss, sp + 4, (f | ZF) if on else (f & ~ZF))

    def _on_stub(self, uc, address, size, user):
        n = (address - (BIOS_SEG << 4)) // 4
        if (address - (BIOS_SEG << 4)) % 4:
            return
        ah = self.rb("ah")
        key = (n, ah)
        self.int_counts[key] = self.int_counts.get(key, 0) + 1
        if self.trace_ints and n not in (0x08, 0x1C, 0x16):
            self.log(f"INT {n:02x} AX={self.r('ax'):04x} BX={self.r('bx'):04x} "
                     f"CX={self.r('cx'):04x} DX={self.r('dx'):04x}")
        handler = getattr(self, f"int_{n:02x}", None)
        if handler is None:
            return  # plain IRET
        handler()

    # INT 08h: BIOS timer tick -> BDA counter, then INT 1Ch.
    def int_08(self) -> None:
        t = (self.u16(0x40, 0x6C) | self.u16(0x40, 0x6E) << 16) + 1
        self.put16(0x40, 0x6C, t & 0xFFFF)
        self.put16(0x40, 0x6E, t >> 16)
        vec = (self.u16(0, 0x1C * 4 + 2), self.u16(0, 0x1C * 4))
        if vec != (BIOS_SEG, 0x1C * 4):
            # Tail-jump into the user's INT 1Ch; its IRET returns for us.
            self.w("cs", vec[0])
            self.w("ip", vec[1])

    # INT 09h: BIOS keyboard IRQ -> translate the last scan code.
    def int_09(self) -> None:
        scan = self.last_scan
        if scan & 0x80:
            return
        for name, (s, a) in KEYS.items():
            if s == scan:
                self.keyq.append((s, a))
                return
        self.keyq.append((scan, 0))

    def int_10(self) -> None:
        ah = self.rb("ah")
        if ah == 0x00:
            self.video_mode = self.rb("al") & 0x7F
            self.write(0x40, 0x49, bytes([self.video_mode]))
            if self.video_mode == 0x13:
                self.write(0xA000, 0, bytes(64000))
            self.log(f"video mode {self.video_mode:02x}")
        elif ah == 0x0F:
            self.wb("al", self.video_mode)
            self.wb("ah", 40 if self.video_mode == 0x13 else 80)
            self.wb("bh", 0)
        elif ah == 0x10 and self.rb("al") == 0x12:
            start, count = self.r("bx"), self.r("cx")
            raw = self.read(self.r("es"), self.r("dx"), count * 3)
            self.dac[start * 3:start * 3 + count * 3] = raw
        elif ah == 0x10 and self.rb("al") == 0x10:
            i = self.r("bx")
            self.dac[i * 3:i * 3 + 3] = bytes([self.rb("dh"), self.rb("ch"), self.rb("cl")])
        elif ah == 0x1A:
            self.wb("al", 0x1A)
            self.w("bx", 0x0008)   # VGA colour
        elif ah == 0x12 and self.rb("bl") == 0x10:
            self.w("bx", 0x0003)
        # other services (cursor, teletype) are ignored

    def int_11(self) -> None:
        self.w("ax", self.u16(0x40, 0x10))

    def int_12(self) -> None:
        self.w("ax", 640)

    def int_15(self) -> None:
        self._set_cf(True)
        self.wb("ah", 0x86)

    def int_16(self) -> None:
        ah = self.rb("ah") & 0x0F
        if ah == 0x00:
            if not self.keyq:
                # Blocking read with nothing queued: rewind to the INT so the
                # program keeps polling while timer ticks advance.
                self._retry_int()
                return
            s, a = self.keyq.pop(0)
            self.w("ax", s << 8 | a)
        elif ah == 0x01:
            if self.keyq:
                s, a = self.keyq[0]
                self.w("ax", s << 8 | a)
                self._set_zf(False)
            else:
                self._set_zf(True)
        elif ah == 0x02:
            self.wb("al", 0)

    def _retry_int(self) -> None:
        # Return address on the stack points after the 2-byte INT instruction.
        ss, sp = self.r("ss"), self.r("sp")
        self.put16(ss, sp, self.u16(ss, sp) - 2)
        self.idle = True

    def int_1a(self) -> None:
        if self.rb("ah") == 0x00:
            self.w("cx", self.u16(0x40, 0x6E))
            self.w("dx", self.u16(0x40, 0x6C))
            self.wb("al", 0)

    def int_33(self) -> None:
        ax = self.r("ax")
        m = self.mouse
        if ax == 0x0000:
            self.w("ax", 0xFFFF)
            self.w("bx", 2)
        elif ax == 0x0001:
            m["shown"] = 1
        elif ax == 0x0002:
            m["shown"] = 0
        elif ax == 0x0003:
            self.w("bx", m["buttons"])
            self.w("cx", m["x"] * 2)
            self.w("dx", m["y"])
        elif ax == 0x0004:
            m["x"], m["y"] = self.r("cx") // 2, self.r("dx")
        elif ax in (0x0005, 0x0006):
            self.w("ax", m["buttons"])
            self.w("bx", 0)
            self.w("cx", m["x"] * 2)
            self.w("dx", m["y"])

    def int_20(self) -> None:
        self._terminate(0)

    def _terminate(self, code: int) -> None:
        self.exit_code = code
        self.log(f"exit code {code}")
        self.stopped = True
        self.uc.emu_stop()

    # INT 21h ---------------------------------------------------------------
    def int_21(self) -> None:
        ah = self.rb("ah")
        fn = getattr(self, f"dos_{ah:02x}", None)
        if fn is None:
            self.log(f"unhandled INT 21h AH={ah:02x} AX={self.r('ax'):04x}")
            self._set_cf(True)
            self.w("ax", 1)
            return
        fn()

    def _ok(self) -> None:
        self._set_cf(False)

    def _err(self, code: int) -> None:
        self._set_cf(True)
        self.w("ax", code)

    def dos_02(self) -> None:
        sys.stdout.write(chr(self.rb("dl")))

    def dos_06(self) -> None:
        if self.rb("dl") == 0xFF:
            if self.keyq:
                s, a = self.keyq.pop(0)
                self.wb("al", a)
                self._set_zf(False)
            else:
                self.wb("al", 0)
                self._set_zf(True)

    def dos_09(self) -> None:
        raw = self.read(self.r("ds"), self.r("dx"), 512).split(b"$", 1)[0]
        self.log("DOS print: " + raw.decode("latin-1"))

    def dos_0b(self) -> None:
        self.wb("al", 0xFF if self.keyq else 0)

    def dos_0e(self) -> None:
        self.wb("al", 3)

    def dos_19(self) -> None:
        self.wb("al", 2)  # C:

    def dos_1a(self) -> None:
        self.dta = (self.r("ds"), self.r("dx"))

    def dos_25(self) -> None:
        n = self.rb("al")
        self.put16(0, n * 4, self.r("dx"))
        self.put16(0, n * 4 + 2, self.r("ds"))
        self.log(f"set vector {n:02x} -> {self.r('ds'):04x}:{self.r('dx'):04x}")

    def dos_2a(self) -> None:
        self.w("cx", 1990)
        self.w("dx", 0x0601)
        self.wb("al", 5)

    def dos_2c(self) -> None:
        t = self.u16(0x40, 0x6C)
        self.w("cx", 0x0C00)
        self.w("dx", (t % 60) << 8)

    def dos_2f(self) -> None:
        self.w("es", self.dta[0])
        self.w("bx", self.dta[1])

    def dos_30(self) -> None:
        self.w("ax", 0x0005)  # DOS 5.0
        self.w("bx", 0)
        self.w("cx", 0)

    def dos_33(self) -> None:
        self.wb("dl", 0)

    def dos_35(self) -> None:
        n = self.rb("al")
        self.w("bx", self.u16(0, n * 4))
        self.w("es", self.u16(0, n * 4 + 2))

    def dos_36(self) -> None:
        self.w("ax", 8)
        self.w("bx", 0x4000)
        self.w("cx", 512)
        self.w("dx", 0x8000)

    def dos_3b(self) -> None:
        self._ok()

    def dos_47(self) -> None:
        self.write(self.r("ds"), self.r("si"), b"EXCAL\0")
        self._ok()

    def dos_3c(self) -> None:
        name = self.asciiz(self.r("ds"), self.r("dx"))
        path = self.resolve(name)
        h = self._new_handle()
        if h is None:
            self._err(4)
            return
        self.files[h] = OpenFile(path, bytearray(), dirty=True)
        self.log(f"create {name} -> {h}")
        self.w("ax", h)
        self._ok()

    def dos_3d(self) -> None:
        name = self.asciiz(self.r("ds"), self.r("dx"))
        path = self.resolve(name)
        if not path.is_file():
            self.log(f"open {name} -> not found")
            self._err(2)
            return
        h = self._new_handle()
        if h is None:
            self._err(4)   # too many open files, like DOS with FILES=20
            return
        self.files[h] = OpenFile(path, bytearray(path.read_bytes()))
        self.log(f"open {name} -> {h}")
        self.w("ax", h)
        self._ok()

    def _new_handle(self) -> int | None:
        # DOS returns the lowest free handle; the C runtime indexes fixed
        # 20-entry tables with it, so never hand out 20 or above.
        for h in range(5, 20):
            if h not in self.files:
                return h
        return None

    def dos_3e(self) -> None:
        f = self.files.pop(self.r("bx"), None)
        if f and f.dirty and self.writes_enabled:
            f.path.write_bytes(bytes(f.data))
        self._ok()

    writes_enabled = False   # saves go to memory unless a script opts in

    def dos_3f(self) -> None:
        h, n = self.r("bx"), self.r("cx")
        f = self.files.get(h)
        if f is None:
            if h == 0:
                self.w("ax", 0)
                self._ok()
                return
            self._err(6)
            return
        chunk = f.data[f.pos:f.pos + n]
        self.write(self.r("ds"), self.r("dx"), chunk)
        f.pos += len(chunk)
        self.w("ax", len(chunk))
        self._ok()

    def dos_40(self) -> None:
        h, n = self.r("bx"), self.r("cx")
        data = self.read(self.r("ds"), self.r("dx"), n)
        if h in (1, 2):
            self.log("stdout: " + data.decode("latin-1").rstrip())
        else:
            f = self.files.get(h)
            if f is None:
                self._err(6)
                return
            end = f.pos + n
            if end > len(f.data):
                f.data.extend(bytes(end - len(f.data)))
            f.data[f.pos:end] = data
            f.pos = end
            f.dirty = True
        self.w("ax", n)
        self._ok()

    def dos_41(self) -> None:
        self._ok()

    def dos_42(self) -> None:
        f = self.files.get(self.r("bx"))
        if f is None:
            self._err(6)
            return
        off = (self.r("cx") << 16) | self.r("dx")
        if off & 0x80000000:
            off -= 1 << 32
        base = {0: 0, 1: f.pos, 2: len(f.data)}[self.rb("al")]
        f.pos = max(0, base + off)
        self.w("dx", f.pos >> 16)
        self.w("ax", f.pos & 0xFFFF)
        self._ok()

    def dos_43(self) -> None:
        name = self.asciiz(self.r("ds"), self.r("dx"))
        if self.resolve(name).exists():
            self.w("cx", 0x20)
            self._ok()
        else:
            self._err(2)

    def dos_44(self) -> None:
        al = self.rb("al")
        if al == 0x00:
            h = self.r("bx")
            self.w("dx", 0x80D3 if h < 5 else 0x0002)
            self._ok()
        elif al == 0x08:
            self.w("ax", 1)
            self._ok()
        else:
            self._ok()

    def dos_48(self) -> None:
        paras = self.r("bx")
        seg = self._alloc(paras)
        if seg is None:
            self.w("bx", self._largest_free())
            self._err(8)
        else:
            self.w("ax", seg)
            self._ok()

    def dos_49(self) -> None:
        self.blocks.pop(self.r("es"), None)
        self._ok()

    def dos_4a(self) -> None:
        seg, paras = self.r("es"), self.r("bx")
        if seg not in self.blocks:
            self._err(9)
            return
        limit = TOP_SEG
        for start in sorted(self.blocks):
            if start > seg:
                limit = start
                break
        if seg + paras > limit:
            self.w("bx", limit - seg)
            self._err(8)
            return
        self.blocks[seg] = paras
        self._ok()

    def dos_4c(self) -> None:
        self._terminate(self.rb("al"))

    def dos_4e(self) -> None:
        pattern = self.asciiz(self.r("ds"), self.r("dx"))
        import fnmatch
        folder, _, mask = pattern.replace("/", "\\").rpartition("\\")
        base = self.resolve(folder) if folder else self.game_dir
        names = sorted(e.name for e in base.iterdir()
                       if e.is_file() and fnmatch.fnmatch(e.name.upper(), mask.upper()))
        self.find_state = [str(base / n) for n in names]
        self._find_next()

    def dos_4f(self) -> None:
        self._find_next()

    def _find_next(self) -> None:
        if not self.find_state:
            self._err(18)
            return
        p = Path(self.find_state.pop(0))
        rec = bytearray(43)
        rec[0x15] = 0x20
        struct.pack_into("<I", rec, 0x1A, p.stat().st_size)
        nm = p.name.upper().encode()[:12]
        rec[0x1E:0x1E + len(nm)] = nm
        self.write(self.dta[0], self.dta[1], rec)
        self._ok()

    def dos_50(self) -> None:
        self._ok()

    def dos_51(self) -> None:
        self.w("bx", self.programs[-1].psp)

    def dos_62(self) -> None:
        self.w("bx", self.programs[-1].psp)

    def dos_57(self) -> None:
        self.w("cx", 0)
        self.w("dx", 0x1421)
        self._ok()

    # IRQ / ports -----------------------------------------------------------
    def _on_in(self, uc, port, size, user):
        if port == 0x60:
            return self.last_scan
        if port == 0x61:
            return 0
        if port == 0x64:
            return 0x14 | (1 if self.scanq else 0)
        if port == 0x3DA:
            # Toggle retrace so wait loops terminate.
            self._retrace = not getattr(self, "_retrace", False)
            return 0x09 if self._retrace else 0x00
        if port == 0x3C9:
            v = self.dac[self.dac_read * 3 + self.dac_sub]
            self.dac_sub += 1
            if self.dac_sub == 3:
                self.dac_sub = 0
                self.dac_read = (self.dac_read + 1) & 0xFF
            return v
        if port == 0x201:
            return 0xFF   # no joystick
        self.port_log[port] = self.port_log.get(port, 0) + 1
        return 0

    def _on_out(self, uc, port, size, value, user):
        if port == 0x3C8:
            self.dac_write, self.dac_sub = value & 0xFF, 0
        elif port == 0x3C7:
            self.dac_read, self.dac_sub = value & 0xFF, 0
        elif port == 0x3C9:
            self.dac[self.dac_write * 3 + self.dac_sub] = value & 0x3F
            self.dac_sub += 1
            if self.dac_sub == 3:
                self.dac_sub = 0
                self.dac_write = (self.dac_write + 1) & 0xFF
        elif port == 0x40:
            self.pit_reload = (self.pit_reload >> 8) | ((value & 0xFF) << 8)
        else:
            self.port_log[port] = self.port_log.get(port, 0) + 1

    # ------------------------------------------------------------------- run
    def run(self, ticks: int = 1) -> None:
        """Run for `ticks` timer periods, delivering IRQ0 between them."""
        for _ in range(ticks):
            if self.stopped:
                raise Halt(f"stopped, exit code {self.exit_code}")
            self.idle = False
            cs, ip = self.r("cs"), self.r("ip")
            try:
                self.uc.emu_start((cs << 4) + ip, 0xFFFFFFFF, count=self.insns_per_tick)
            except UcError as exc:
                cs, ip = self.r("cs"), self.r("ip")
                raise RuntimeError(f"CPU fault at {cs:04x}:{ip:04x}: {exc}") from exc
            if self.stopped:
                raise Halt(f"stopped, exit code {self.exit_code}")
            self.ticks += 1
            self._deliver_irqs()

    def _deliver_irqs(self) -> None:
        # Like the PIC, hold the IRQ pending until the CPU has IF set.
        guard = 0
        while not self.r("flags") & IF:
            cs, ip = self.r("cs"), self.r("ip")
            # Odd step so we cannot phase-lock with a polling loop whose
            # length divides the step and always stop inside an INT stub.
            self.uc.emu_start((cs << 4) + ip, 0xFFFFFFFF, count=7 + guard % 5)
            if self.stopped:
                return
            guard += 1
            if guard > 20000:
                return  # interrupts stay masked; drop this tick
        if self.scanq:
            self.last_scan = self.scanq.pop(0)
            self.raise_int(0x09)
            return
        self.raise_int(0x08)

    # --------------------------------------------------------------- scripts
    def press(self, name: str) -> None:
        """Queue a key press and release (port 60h + INT 16h)."""
        scan, _ascii = KEYS[name.lower()]
        self.scanq += [scan, scan | 0x80]

    def move(self, x: int, y: int) -> None:
        """Put the mouse at logical (x, y)."""
        self.mouse["x"], self.mouse["y"] = x, y

    def click(self, x: int, y: int, hold: int = 4, after: int = 8, button: int = 1) -> None:
        """Move, hold a button for `hold` ticks (the game polls INT 33h/03),
        release and run `after` more ticks."""
        self.move(x, y)
        self.run(12)    # let the game see the cursor over the target first
        self.mouse["buttons"] = button
        self.run(hold)
        self.mouse["buttons"] = 0
        self.run(after)

    def type(self, text: str) -> None:
        for ch in text:
            self.press("space" if ch == " " else ch.lower())

    def screen(self) -> tuple[bytes, bytes]:
        """Mode 13h framebuffer and 8-bit RGB palette."""
        pixels = bytes(self.uc.mem_read(VGA_LINEAR, 64000))
        pal = bytes(min(255, v * 255 // 63) for v in self.dac)
        return pixels, pal

    def save_png(self, path: Path) -> None:
        from PIL import Image
        pixels, pal = self.screen()
        img = Image.frombytes("P", (320, 200), pixels)
        img.putpalette(pal)
        img.save(path)

    def snapshot(self) -> dict:
        regs = {n: self.r(n) for n in REGS16}
        return {
            "mem": bytes(self.uc.mem_read(0, MEM_SIZE)), "regs": regs,
            "dac": bytes(self.dac), "ticks": self.ticks, "blocks": dict(self.blocks),
            "files": {h: (str(f.path), bytes(f.data), f.pos) for h, f in self.files.items()},
            "keyq": list(self.keyq), "scanq": list(self.scanq), "mode": self.video_mode,
            "programs": [(p.psp, p.name) for p in self.programs], "dta": self.dta,
            "next_handle": self.next_handle, "mouse": dict(self.mouse),
        }

    def restore(self, snap: dict) -> None:
        self.uc.mem_write(0, snap["mem"])
        for n, v in snap["regs"].items():
            self.w(n, v)
        self.dac = bytearray(snap["dac"])
        self.ticks = snap["ticks"]
        self.blocks = dict(snap["blocks"])
        self.files = {h: OpenFile(Path(p), bytearray(d), pos) for h, (p, d, pos) in snap["files"].items()}
        self.keyq, self.scanq = list(snap["keyq"]), list(snap["scanq"])
        self.video_mode = snap["mode"]
        self.programs = [Program(psp, name) for psp, name in snap["programs"]]
        self.dta = snap["dta"]
        self.next_handle = snap["next_handle"]
        self.mouse = dict(snap.get("mouse", self.mouse))
        self.stopped = False


def main(argv: list[str]) -> int:
    import argparse
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("program", nargs="?", default="EXCAL.EXE")
    ap.add_argument("--ticks", type=int, default=200)
    ap.add_argument("--png", type=Path)
    ap.add_argument("--trace-ints", action="store_true")
    args = ap.parse_args(argv)
    m = DosMachine(log=print, trace_ints=args.trace_ints)
    m.load(args.program)
    try:
        m.run(args.ticks)
    except (Halt, RuntimeError) as exc:
        print(exc)
    print(f"ticks={m.ticks} cs:ip={m.r('cs'):04x}:{m.r('ip'):04x} mode={m.video_mode:02x}")
    if args.png:
        m.save_png(args.png)
        print("wrote", args.png)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
