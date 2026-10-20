# Driver notes

## MCGA.DVR

- Original file: `MCGA.DVR`, 5238 bytes (user-supplied, never committed).
- Not yet reversed. The `Video` trait in `soe-video` is therefore provisional.

What the executable shows today:

- `INT 10h` mode 3 (text) from `1316:0002` / `1316:0365`.
- `INT 10h` mode 4 and cursor positioning from `307a:0353`.
- VGA status port reads from `0x3DA` in `1316:1302`.
- Game code uses logical 320x200 coordinates; presentation-time palette effects
  are not yet mapped to driver calls.
- No evidence yet of screen readback. This remains an open question; if the
  original uses save-under menus or screen copies, `Video` needs a logical
  readback path.

## Other drivers

`EGA.DVR`, `TAN.DVR`, `TANDY.DVR`, `IBM.DVR`, `ADLIB.DVR`, `ROL.DVR`,
`CMS.DVR`, `COVOX.DVR`, `NULL.DVR` are present in the game folder. They are not
reversed yet. Sound and Input traits are placeholders until their boundaries are
known.


## 2026-10-20 driver import and analysis

Imported into the SOE Ghidra project as raw `x86:LE:16:Real Mode` binaries at
base `0000:0000`:

- `MCGA.DVR`: 10 stubs at file offset `0x7a`, each a 3-byte `E9` near jump.
- `ADLIB.DVR`: 13 stubs at `0x32`, each 10 bytes:
  `CALL prolog; CALL body; CALL epilog; RETF`.
- `IBM.DVR`: 13 stubs at `0x32`, same 10-byte form.

The EXE loader `1316:03c2` sets the driver header pointer to loaded base +
`0x2a` and installs 13 stubs at `0x32 + n*0x0a` through `0xaa`. That matches
ADLIB/IBM but not MCGA; the exact MCGA video-loading path is still unresolved
(possibly START.EXE or a separate video loader). See `notes/MCGA_API.md`.

Exports (git-ignored) are under `export/drivers/MCGA`, `export/drivers/ADLIB`
and `export/drivers/IBM`.
