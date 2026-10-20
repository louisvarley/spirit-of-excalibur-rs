# Main loop and scene renderer

Status: **structure confirmed** from `2d65:2cd9` and `2b79:0003`; individual
subsystems remain unported.

## Startup

`entry` (`1000:0000`) runs the Turbo C startup and calls `FUN_2d65_2cd9`. The
latter:

1. Installs handlers / driver callbacks.
2. Allocates buffers with `FUN_14b0_0122(200)` and `(300)`.
3. Sets the palette selector `DS:8E5D = 0x1FD9`, active buffer `DS:9BA0`.
4. Registers driver entry points at `DS:8A43`, `DS:8AB6`, `DS:8E19`...
5. Loads a save with `FUN_1a51_059d(DS:798E, DS:7990, DS:8E3D)`.
6. Runs an infinite state loop on `DS:01EC`:
   - `-5`: `FUN_1316_1143`, `FUN_2c74_0006(0)`, state becomes `-2`
   - `-4`: "GAMELOST", `FUN_2c74_0c2e(0x0f)`, state becomes quit
   - `-3`: `func_0x0003d651`
   - `-1`/`-2`: normal frame tick / title or idle path
   - otherwise: play/render frame.

The VGA mode is set through the driver callback at `DS:8E65` (MCGA or EGA/Tandy
variant).

## Scene renderer (`FUN_2b79_0003`)

- Object array pointer: `DS:9D14`; count: `DS:9D18`.
- Per object: id = `u32 & 0x7FF`; x = `(next_word & 0x1FF) - 190`;
  y = `word & 0xFF`; flip = `flags & 1`.
- Animation ids: `700..=799` and `900..=999`; every other id uses the static
  image path.
- Static: `FUN_14b0_0122(id)` gives header `{... width +8, height +10}`.
- Animation: `FUN_14b0_0251(id, 0)` looks up `FRML` by id, advances a frame
  counter, fixed height 20, width from frame metadata.
- Blit flags are `0x46 | flip`; `FUN_14b0_0bbf` folds bit0 into the sprite
  control word (`control ^= (flags & 1) << 7`), confirming horizontal mirroring.
- Id `1000` sets `local_a = -0x18` (special boundary/culling case).
- The renderer also cross-fades cached frames when `DS:9D19 != 0`.

## Room compositor mapping

`soe-engine::room` currently implements the static path: signed x, top-edge y,
palette index 0 transparent, file-order drawing, `flags & 1` horizontal flip.
Animation frames and the special 1000 case are not yet ported.
