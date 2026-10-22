# Start-up intro

`FUN_2d65_2cd9` calls `FUN_2c74_0006(1)` at `2d65:2e3c`, after the start-up
loaders and before the menu. With argument 0 the same routine is the ending
("As %S takes the eldritch blade…", `DS:78C8`); only argument 1 is ported.
Rust: `soe-engine::intro::Intro`, run by `Runtime::start_intro` as
`Screen::Intro`; any key or click skips it, then `Screen::Menu`.

There are no publisher/developer logo screens in this path: the game shows
the lake straight after the loaders. `LOGO.RES` is not used by the intro.

## Sequence (game ticks are `DS:7E3E`, 18.2 Hz)

| step | what | timing |
|---|---|---|
| start | music `"title"` (`FUN_1316_187b`), lake `TITLE#0x78` to screen and work buffer, `DS:9D30 = 1` (cycling, VGA only), fade in `DS:1FD9` | driver fade |
| hold | `FUN_2c74_0b31(0x1E0)` | 480 ticks |
| rise | i = 3..0xB1 step 6: blade `#0x76` rises `i` rows above y 0xB5, wake `#0x73/74`, hand `#0x6F..72` from i > 0x78 | 3 ticks each |
| ripples | 2 x `FUN_2c74_0add` (ripple strip `#0x6B..6D` at 0x5D,0xB5) | 3 x 8 ticks each |
| splash | sound 7 (level 0x28); 12 steps, splash `#100..106,103..100` at `DS:73AC/73B7` | 4 ticks each |
| ripples | 2 x `0add` | |
| storm | 20 steps, lightning `#0x7B..7E` per `DS:73C2` with sound 0x0B (level 0x3C); step 7: title `#0x77` into the work buffer at (0x14, 7), flash: DAC all (0xE,0xF,0xE), full copy, fade back in; step 15: 2 x `0add` | 6 ticks each |
| linger | 5 x `0add` | |
| sink | i = 0xB1 down to 3 step 6, blade sinks | 3 ticks each |
| end | work buffer (lake + title) to screen, `0b31(0x168)`, fade to black (`FUN_1316_1143`) | 360 ticks |

About 1450 ticks (80 s) plus fades.

## Callees

- `FUN_2c74_0b31(n)`: wait until clock + n. Every pass: if `DS:9D30` and
  the clock is past `DS:9D1C` (and sound type `DS:7D2A[3] != 6`), set
  `9D1C = clock + 15` and rotate colours 11..15 of the table at `DS:1FD9`
  up by one (`FUN_32a2_0059` moves), then `FUN_1316_1302` sets the DAC. A key
  (`FUN_3360_0000(1)`) or click (`DS:7D3E`) sets `DS:9DBB`, which ends the
  wait and every remaining loop. The code between loops still runs, so a
  skip still copies the work buffer to the screen before the fade out.
- `FUN_2c74_0add`: three ripple frames to the screen, `0b31(8)` after each.
- `FUN_14b0_0bbf(src, sx, sy, dst, dx, dy, w, h, remap, flags)`: rectangles
  are (top, left, bottom, right). Unless flags have 0x10/0x20, or 0x40 with
  source header byte 0 bit 7, both rectangles widen to 8-pixel columns:
  `left &= ~7`, `right = (right + (left & 7) + 7) & ~7`. Then the source is
  clipped to the image (flag 4), the destination to its surface, and both
  re-derived from the shared offset. Flag 0x40 skips index 0.
- `FUN_1316_1111`: MCGA.DVR `drv_8`: target = table through the driver's
  4-bit→6-bit table (`CS:0900`, = `(v*63+7)/15`), then `09c3`: per vertical
  retrace every component moves 2 towards the target (snaps when within
  1), at most 0x40 passes. `FUN_1316_1143`: `drv_7(0,0,0)`, the same fade to
  black.
- `FUN_1316_1302`/`1169` on VGA (`DS:7D2A[0] == 4`) write the DAC directly
  with `4v + v/4` (not the driver table: colour 3 gives 12, not 13).

## Verification

`tests/oracle/intro_vectors.py` boots the original and records the screen
and DAC at every `0b31` entry and after every cycle step, plus the entry
clock, `DS:9D1C` and DAC (`tests/vectors/intro/`). The Rust test
`intro::tests::matches_original_intro` replays the intro with fades that
take no game time (the emulator's `3DA` toggles on every read, so its fades
are instant too) and compares every frame, the DAC and the clock offsets.

## Unverified

- Fade speed in real time: the Rust build steps fades at 70 Hz (3.85 passes
  per tick); the oracle cannot time retraces.
- Sounds 7 and 0x0B are emitted as events but not played (no sound-effect
  playback yet). The music is the `"title"` piece from `MUSIC.RES`.
- The argument-0 (ending) branch.
