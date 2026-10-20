# STATUS

Updated: M1/M2/M3 foundation session.

## Read this first

- `CLAUDE.md` ground rules and architecture invariants are authoritative.
- Oracle: `tests/oracle/excal_oracle.py` (Unicorn real mode). LZSS and RLE
  routines verified; vectors in git-ignored `tests/vectors/`.
- Prior repo hypotheses live in git-ignored `reference/prior/`.

## Done

- Workspace, `soe-core`, `soe-res`, `soe-assets`, `soe-video`, `soe-tools`,
  `soe-engine`.
- `.RES` header/type/directory schema verified; 841 records audited.
  LZSS 523/523 oracle-exact; flag-0x01 RLE layout verified.
- 5-plane image format verified (height,width,planes; even stride).
- Palettes: room `0x2099`, map `0x20F9`, UI `0x2159`, expansion `v*17`.
- `soe img dump` and `soe room compose`.
- Save/EPISODE loader and writer with the exact DGROUP mapping.
- SCEN schema and room compositor; all 138 scenes parse/compose; object flip
  confirmed.
- FRML fully decoded: 2-byte header, big-endian offsets, raw planar frames.
- `FUN_14b0_0451` formatter, oracle-verified for the text subset.
- Game font atlas located: `TITLE.RES#200` + EXCAL metrics; `BitmapFont`.
- Headless `Input`, `Timer`, `Sound`, `GameFiles` (read/write/list),
  `Platform`, `ResourceManager`, `Session`.
- Playable `soe-play`: winit window, 18.2 Hz room animation, HUD text,
  keyboard/mouse scene navigation.
- Main loop/renderer structure documented.

## M1 remaining

- Animation timing/clock integration (frame cycling is implemented).
- MMAP records.
- START.EXE driver/load path.
- TITLE/LOGO palette code reference (room palette is visually confirmed for
  `TITLE#122`).
- Exact save-loader oracle diff needs DOS INT 21h emulation.

## M2 remaining

- OPL2/SMUS audio backend.
- Full DOS INT 21h file semantics.

## M3 remaining

- Port `FUN_2d65_2cd9` state loop and driver callback layer.
- Text/UI rendering through `Video` (BitmapFont exists; driver-font path open).
- Screen transitions, fades, palette animation, culling details.

## M4-M7

Not started: combat, trade, magic, dialogue, travel, map screens, save/load UI,
integration, packaging, upscale/override pipeline.

## Next

1. Port the main-loop state machine and dialogue/UI panels.
2. M2 OPL2/SMUS audio backend.
3. Map/travel screens and save/load UI.
4. M4 game systems.
