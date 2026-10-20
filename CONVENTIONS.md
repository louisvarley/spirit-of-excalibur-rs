# Conventions for this binary

Facts marked (unverified) come from reading exports and have not been tested yet. Move them to verified once a trace test confirms them.

## Executable

- `EXCAL.EXE` is LZEXE 0.91 packed (86.54 KiB); `EXCAL_UNPACKED.EXE` (184.50 KiB) is what Ghidra analyses.
- Compiler: Turbo C, 1988 (the string `Turbo-C - Copyright (c) 1988 Borland Intl.` is at DS:0004). Plain C, no C++.
- Memory model: **large**. Far calls (`CALLF`/`RETF`) dominate: about 2086 CALLF vs 255 near CALL in the listing.
- 86 code segments, one per source module, packed at paragraph boundaries (not 64K-aligned), then DGROUP.
- No `INT 34h-3Dh` (no emulated floating point). Real FPU instructions have not yet been ruled out (unverified).
- Interrupts used: `10h` video, `12h` memory size, `16h` keyboard, `1Ah` timer ticks, `21h` DOS, `33h` mouse. No audio interrupt; sound goes through port I/O or a driver (unverified).

## Addresses

- Ghidra loads the image at `1000:0000`. The MZ header is a 10752-byte block of its own and contains about 2600 relocations.
- Image offset = `seg*16 + off - 0x10000`.
- Runtime segment = `load_segment + (ghidra_segment - 0x1000)`, where `load_segment` is the PSP segment + 0x10. Use this when mapping Ghidra addresses onto DOSBox-X traces.
- Ghidra prints near branch targets and `CALLF` targets in canonical form (e.g. `CALLF 0x1000:724a` is really `15b8:16ca`). The export appends `; call -> NAME @ seg:off` or `; jump -> seg:off` with the real targets. Raw bytes are authoritative.

## Segments

- `1000` (1392 bytes): startup and tiny runtime helpers. Entry at `1000:0000`.
- `1057`, `105e` (7.5 KB): look like the Turbo C runtime and library (unverified).
- `1ba8` is the largest module (13 KB, 44 functions). `18cb` is the handler/dispatch module for the function-pointer table at DS:0389.
- DGROUP is segment `3364`: `3364:0000-81bb` initialised data (strings, tables), `3364:81bc-b96b` uninitialised (BSS and stack).

## Calling convention (from the disassembly of `18cb:0944`, `2d65:0ac8`)

- cdecl, far: arguments pushed right to left, caller removes them (`ADD SP,n`).
- Far functions: first argument at `[BP+6]` (return address 4 bytes + saved BP 2). Near functions: `[BP+4]`.
- Frame: `PUSH BP; MOV BP,SP; SUB SP,n` ... `MOV SP,BP; POP BP; RETF`. Locals at `[BP-n]`.
- Return value: `AX` for 16-bit; `DX:AX` for 32-bit and far pointers (DX = segment/high word, AX = offset/low word).
- SI and DI are used as register variables and should be preserved by callees (unverified).
- Globals are DS-relative (`DS = 3364` during game code). Far pointers in globals are loaded with `LES BX,[addr]`. Example: `DS:9a6e` is a far pointer to the current record (flags at +0x24, fields at +0x10/+0x12/+0x16/+0x1e).

## Switch statements

- Dense switch: `CMP AX,n; JBE; JMP default; MOV BX,AX; SHL BX,1; JMP word ptr CS:[BX + table]`, with the table of 16-bit segment-relative offsets in the code segment immediately after the `JMP`. First case code follows the table, so `N = (entry0 - table)/2`; the `CMP` bound agrees.
- Sparse switch: expected to be a value table scanned with `CMP AX,CS:[BX]` and `LOOP`, then a target table (unverified, being checked).
- Ghidra needs `ResolveCsSwitchTables.java` run on the project; without it, switch functions are truncated and decompile as garbage.

## Data in DGROUP (from string spacing, unverified field layouts)

- Character/entity records: stride `0x38` (56 bytes); names inline near the start (e.g. `3364:2c96` Bedivere, `2cce` Ector).
- Location records: stride `0x19` (25 bytes) from about `3364:40cf`.
- Item records: stride `0x1b` (27 bytes) from about `3364:48a6`.
- A 4-byte-stride flags table around `3364:4dd2` (85 entries).
- Two far-pointer tables at `DS:0389` (16 entries, all in segment 18cb) and `DS:79a6` (3 entries). See `notes/FUNC_POINTERS.md`.

## Strings that identify subsystems

- Resource manager: `Resource not found:`, `FindBlock: insuf fixd, req:`, `FindBlock: insuf rel, avail:`. Resource names include `BTLSTRT`, `BTLLOST`, `GAMEWON`, `GAMELOST`, `CAMELOT`, `STNHENG`, `CATHEDRL`, `GOODCSTL`, `MORGCSTL`, `ENCHLAB`, `RUINS`, `CSTLROOM`, `VILLAGE1`, `VILLAGE2`, `title`.
- Data files: `game.cfg`, `EPISODE1.DAT` to `EPISODE5.DAT`.
- Text formatter: printf-like with custom codes `%S`, `%W`, `%R`, `%N`, `%l`.
- Built-in copy protection (shield-matching question, "square medallion" prompt). The port needs a bypass.

## Open questions

- Which functions are the resource loader and `INT 21h` file I/O wrappers?
- Sound path: which ports, which driver files, is there a separate driver load?
- Video modes used via `INT 10h` (EGA, Tandy, VGA?).
- Do any real FPU opcodes appear?
- Which functions are the Turbo C runtime (long multiply/divide/shift helpers, printf family)?


## Corrections from follow-up 9

- `1057:0000` is the LZSS decompressor, called only by `1237:0904`. The loader
  uses `0x30d5:0110` for the secondary flag-0x01 RLE codec.
- `.RES` directory records are 12 bytes starting with `u16 id`, then
  `u16 0xFFFF`, `u24 offset`, `u8 flags`, `u32 extra`; payload base is `0x18`
  (6-dword header) or `0x14` (SCENES 5-dword header).
- MCGA.DVR uses 10 x 3-byte `E9` stubs at `0x7a`, unlike ADLIB/IBM's 13 x
  10-byte CALL/RETF stubs at `0x32`. The EXE 0x32 loader is the sound path.
