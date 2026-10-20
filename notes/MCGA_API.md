# MCGA.DVR API

Status: **best-effort / mostly unverified**. The driver was imported as a raw
`x86:LE:16:Real Mode` binary at base `0000:0000` and analysed. Stubs are at
`0x7a`; they are 10 x 3-byte `E9` near jumps (not the 10-byte CALL stubs used
by the sound drivers).

## Loader facts (from EXCAL_UNPACKED.EXE)

`FUN_1316_03c2` loads a configured driver file into an allocated segment,
reads its size, and builds a request/API block. For the 10-byte-stub drivers it
sets:

- header pointer = loaded base + `0x2a`
- stub 0 = base + `0x32`
- subsequent stubs every `0x0a` bytes, through base + `0xaa` (13 entries).

That matches ADLIB.DVR/IBM.DVR exactly (13 stubs at `0x32`). MCGA.DVR does
**not** fit this layout: its stub table is at `0x7a` and has 10 entries. The
video driver path therefore differs from the sound-driver path; the exact
video loader was not found in the EXE (possibly in START.EXE / another
executable). This is an open question.

Wrapper call sites use `CALLF [DGROUP:0x8xxx]`; the driver pointers are
installed by the loader. The calls pass `0x1316` (the caller's segment) and
read image/request parameters from the driver's own data block at
`base + 0x2a` and following fields.

## Entry points (MCGA.DVR)

| # | stub | body | best-guess name | call site(s) | confidence |
|---:|---|---|---|---|---|
| 0 | `007a` | `0203` | `blit_image` (clipped, transparent colour 0, optional palette remap) | `1316:0693` | medium |
| 1 | `007d` | `0618` | `set_mode` (INT 10h; `A000` segment) | `1316:06d5` | high |
| 2 | `0080` | `0659` | `draw_text` (NUL-terminated string, newline handling) | `1316:0779` | medium |
| 3 | `0083` | `07f8` | `noop` / `wait` | `1316:0814` | low |
| 4 | `0086` | `12e4` | `decode/blit_planar` (bit depth 8/16/32, header-driven) | `1316:0d87` | medium |
| 5 | `0089` | `1020` | `blit_image_clipped` (header subtype table, clipping) | `1316:0d87` | medium |
| 6 | `008c` | `0805` | `put_pixel` (XOR if DH != 0) | `1316:0d87` | high |
| 7 | `008f` | `0916` | `fade_palette` (three component values, 32-entry buffer) | `1316:0d87` | medium |
| 8 | `0092` | `0999` | `set_palette` (0x60-byte RGB buffer) | `1316:0d87` | high |
| 9 | `0095` | `0aa6` | `draw_masked_image` (bit depth/compare modes) | `1316:183c` | medium |

Helpers: `0963 read_palette` reads 0x60 bytes from DAC `3C7/3C9`;
`09c3 write_palette/fade` writes 0x60 bytes to `3C8/3C9` and performs a
component-wise fade loop.

## Answers to (a)-(f)

**(a) Does the driver draw primitives itself?** Yes. It writes the `A000`
framebuffer directly and has pixel, blit, text, fill/copy-like and palette
entry points. It is not a framebuffer-only surface.

**(b) Screen readback / region copy?** Palette readback exists (`3C7/3C9`).
A full screen readback or save-under region-copy entry point is **not
verified**; it remains an open question for the compositor.

**(c) Clipping.** Yes. `blit_image`/`blit_image_clipped` compute bounds and
clip source/destination; the exact edge rule is unverified.

**(d) Palette programming and fades.** VGA DAC ports `3C8/3C9` for writes,
`3C7/3C9` for reads, with VGA status waits on `3DA`. The routines operate on a
0x60-byte buffer (32 RGB entries) and step component values by ±2 during fades.
Whether the game also loads a full 256-entry palette elsewhere is unverified.
MCGA/VGA DAC values are 6-bit; the driver's intermediate table appears to be
8-bit.

**(e) Text and fonts.** Entry 2 is a text/string loop; the font source and
glyph format were not fully reversed. Font rendering is provisional.

**(f) Compositing/masking/transparency.** Colour 0 is used as transparent in
the blit path (`if (*source != 0)`). `put_pixel` has an XOR mode selected by
`DH`. `draw_masked_image` has bit-depth/compare modes (8/16/32). Exact masking
semantics are unverified.

## Verified set_mode and request block (2026-10-20)

`drv_1` (`0618`) is verified: `INT 10h AX=0x13` (mode 13h, 320x200, 256-colour)
then `INT 10h AX=0x1010, BX=0x000d` sets DAC entry 13 to black; it stores
`A000` as the framebuffer segment. Resolution/colour depth in use: **320x200,
8-bit indexed, 256-entry VGA DAC**.

The driver request block starts at base + `0x2a`. From `drv_0`/`drv_5` reads
(marked unverified where noted):

| offset | size | meaning | confidence |
|---|---:|---|---|
| +0x00 | 4 | far pointer to source/image record | medium |
| +0x04 | 2 | destination x / row base (unverified) | low |
| +0x06 | 2 | destination y / stride (unverified) | low |
| +0x0c | 2 | source x (unverified) | low |
| +0x0e | 2 | source y (unverified) | low |
| +0x1a | 2 | width/clip bound (unverified) | low |
| +0x1e | 2 | height/clip bound (unverified) | low |
| +0x20 | 2 | destination offset (unverified) | low |
| +0x22 | 2 | destination offset (unverified) | low |
| +0x32 | 2 | flags: bit0/1/2, 0x30, 0x40, 0x100, 0x200, 0x4000, 0x8000 | medium |

The call sites (`1316:0693`, `06d5`, `0779`, `0814`, `0d87`, `183c`) use
`CALLF [DGROUP:0x8xxx]`. The driver-loading path for MCGA is still unresolved;
the sound-driver loader at `1316:03c2` uses 0x32/13 stubs and does not match
MCGA's 0x7a/10-jump table.

## Follow-up 9: load path and palettes

- `set_mode` verified: `INT 10h AX=0x13` (320x200, 8-bit indexed, 256 DAC),
  then `AX=0x1010, BX=0x000D` sets DAC entry 13 black.
- Palette copies use DS `0x2099` (rooms), `0x20F9`, `0x2159`; code writes the
  selected pointer to DS `0x8E5D`. DAC writes are 0x60 bytes (32 entries).
- MCGA's 10 x 3-byte E9 stubs at `0x7a` remain different from ADLIB/IBM's 13 x
  10-byte stubs at `0x32`. The EXE loader at `1316:03c2` is the sound-driver
  path; the MCGA video load path (and whether START.EXE is involved) is still
  **unverified**.
- Request block at base+`0x2a`: far pointer to image at +0, dimensions/flags
  around +4/+6/+0x1a/+0x1e/+0x20/+0x22, flag word at +0x32. Field meanings
  remain provisional.


## Follow-up 10 status

- MCGA load path: **UNVERIFIED**. `START.EXE` exists (20,557 bytes) but was not
  unpacked/imported in this pass. The DS variable holding the video driver far
  pointer is still unidentified; the sound loader at `1316:03c2` is not it.
- Font/text: **UNVERIFIED**. Entry 2 (`0080/0659`) draws text; the glyph source,
  size/layout, character range and whether it is driver-internal or a `.RES`
  resource were not established.
- Scene renderer (prior image offset `0x1E389`, widget library `0x5B8`) was not
  verified.


## START.EXE status (M1b)

`START_UNPACKED.EXE` imported and analysed as a separate SOE program (192
functions, export under `export/START`). It is a Turbo C program with DOS file
I/O (open/read/seek/close, find first/next) and a joystick-centering message.
It contains no literal `GAME.CFG`, `.DVR`, `EXCAL` or driver-name strings, so
its configuration/driver job is data-driven. It does not prove the MCGA load
path; that remains **UNVERIFIED**.
