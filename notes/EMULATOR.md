# Headless emulator for the original game

`tests/oracle/dosemu.py` runs the user's own `game/EXCAL.EXE` (packed, as
shipped) under Unicorn with a minimal PC + DOS. `tests/oracle/play.py` drives
it from the command line. Screenshots and snapshots are written to the
git-ignored `tests/vectors/emu/` because they contain original memory/images.

```
python tests/oracle/play.py boot save menu
python tests/oracle/play.py --from menu click 90 29 run 700 shot story save story
```

## What is emulated

- MZ loader with PSP, environment and relocations (the EXCAL unpacker runs as
  normal; the program lands at runtime segment `0x214`, so Ghidra address =
  runtime segment - `0x214` + `0x1000`).
- Real-mode interrupt dispatch through the IVT. Every vector initially points at
  `F000:n*4`, a one-byte `IRET` stub; Python handles the service before the
  `IRET` runs, and flags returned to the caller are patched on the stack.
- DOS: memory (`48/49/4A`), files (`3C-42`, `43`, `44`, find `4E/4F`, DTA),
  vectors (`25/35`), version, date/time, exit. Paths are resolved
  case-insensitively under `game/`; `\EXCAL\` is mapped to `game/`. File writes
  stay in memory unless `writes_enabled` is set.
- BIOS: timer tick (`08`, chains to `1C`), keyboard (`09`, `16`), video (`10`:
  mode set, DAC block/single), equipment/memory size, `1A` tick count.
- Mouse (`33`): reset, show/hide, status (`03`), set position. The vector stub
  starts with `NOP` so the game's "vector is not IRET" driver check succeeds.
- Ports: VGA DAC `3C7/3C8/3C9`, retrace `3DA` (toggles), keyboard `60/64`,
  PIT `40` reload, joystick `201` (absent).
- Timer: IRQ0 every `insns_per_tick` instructions (default 40 000), so runs
  are deterministic. IRQs are held pending until IF is set, like the PIC.

## Findings while bringing it up

- DOS must hand out the **lowest free file handle**. Turbo C's runtime indexes
  fixed 20-entry tables by handle; handles >= 20 overwrite its data and
  corrupted the resource list walked by `FUN_1237_04d0`.
- IRQ delivery must wait for IF. Short catch-up steps must not be a multiple of
  the game's 64-instruction `INT 16h/01` poll loop, or every step stops inside
  the INT stub with IF clear and the IRQ never arrives.
- The menu polls the mouse only every ~20 ticks and wants to see the cursor
  over a button before the press, so `click()` hovers 12 ticks and holds the
  button 40 ticks by default.
- Video and sound drivers come from `GAME.CFG` (`MCGA.DVR`, `IBM.DVR`); EXCAL
  loads them itself. `START.EXE` is not needed to boot.

## Not done yet

- Sound output (PC speaker / AdLib ports are ignored).
- Copy-protection question ("Please insert square medallion marked %d") has not
  been reached yet; answering it needs the user's manual/medallion.
- Speed: roughly 10-40 game ticks per second depending on how much the game
  polls through INT stubs.
- Side-by-side comparison against the Rust port (same input, diff screen and
  DGROUP state).
