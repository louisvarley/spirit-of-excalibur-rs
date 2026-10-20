# Spirit of Excalibur — Rust reimplementation

Goal: a faithful Rust reimplementation of *Spirit of Excalibur* (Synergistic / Virgin-Mastertronic, 1990, DOS) that loads the player's own original game files. No original game files, converted assets or original code are committed to this repo.

## Ground rules

1. **The original binary is the spec.** Disassembly is ground truth. Ghidra's decompiled C is a hint and is sometimes wrong, especially around switch statements, far pointers and long arithmetic. When they disagree, the disassembly wins.
2. **A function is done only when it matches the original under the oracle.** `tests/oracle/` runs the original function from the unpacked EXE in an emulator; the Rust port must produce the same return values and memory writes on the same inputs (golden vectors in `tests/vectors/`). "It compiles and looks right" is not done.
3. **Preserve 16-bit semantics.** Use `u16`/`i16` with explicit wrapping operations; use `u32`/`i32` for DX:AX values. Do not let Rust's integer widening or overflow panics hide original behaviour. Watch signed vs unsigned compares (JL/JG vs JB/JA), SAR vs SHR, and segment:offset arithmetic.
4. **Faithful first, clean later.** Do not refactor until a function matches the original. Refactors go in separate commits.
5. **Never edit `original/`.** Never commit original game files, unpacked executables, converted assets, or anything derived from them. `original/`, `game/`, `export/` (disassembly and decompiled code), the Ghidra project files and `tests/vectors/` are git-ignored. Only our own code, `ghidra/scripts/` and `notes/` are committed. Notes may record addresses, names, struct layouts and findings; do not paste disassembly or decompiled code into them.
6. **Small units.** One function (or one tightly related group) per commit. Commit message: `port: FUN_xxxx_yyyy <name> (verified: <test>)`.
7. **When unsure, say so.** Record open questions in `notes/CONVENTIONS.md` instead of guessing.

## Architecture invariants (keep graphics upscaling possible)

The user will later export every original image, run it through an AI upscaler, and load the results as an override pack. Nothing built now may make that need a rewrite. Check every change against these:

1. **Logical coordinates only.** Game logic and `soe-engine` use the original logical resolution (320x200) and never know the output resolution. No output-size constants outside `soe-video`.
2. **Images are referenced by `ResourceId`** through `AssetSource`. Code outside `soe-assets` and `soe-video` never holds raw pixel buffers of original images and never indexes into them.
3. **The `Video` trait takes image handles/IDs and logical rects, not pixel slices.** If the original reads pixels back (save-under, screen copy), that goes through one documented readback API.
4. **Palette operations stay presentation-time.** Keep images indexed; never convert to RGBA early. Fades, cycling and swaps are applied when presenting.
5. **No hard-coded 320/200** outside `soe-video` and display-mode setup.
6. **Per-image metadata** (palette, transparency, frame/strip layout) lives in `soe-assets`, so tools can export it.
7. **Text goes through a text API** that allows glyph-level or whole-string replacement later. Game logic never rasterises text itself.

Before committing anything graphics-related, ask: could an HD override pack replace this image without changing this code? If not, redesign it.

## Layout

- `original/` : the user's untouched game files and hashes (git-ignored)
- `game/` : game files used for running and testing (git-ignored)
- `ghidra/` : Ghidra project (git-ignored) and `ghidra/scripts/` (committed, e.g. `ExportForAI.java`, `ResolveCsSwitchTables.java`)
- `export/` : generated per-function dumps; regenerate after renaming in Ghidra (git-ignored)
- `notes/` : `CONVENTIONS.md`, `SYMBOLS.md`, `FUNC_POINTERS.md`, `GAPS.md`, `COVERAGE.md`, `DRIVERS.md`, `SAVESTATE.md`, `COMPRESSION.md`
- `port/` : the Rust reimplementation
- `tests/oracle/` : emulation oracle (Python + Unicorn) that runs original functions
- `tests/vectors/` : generated golden vectors (git-ignored)

## Addresses

Everything is identified by Ghidra's `segment:offset` (e.g. `18cb:0944`). Ghidra loads the image at `1000:0000`. Raw bytes are authoritative; the export appends real targets after `;` on calls and jumps. See `notes/CONVENTIONS.md` for the mapping to runtime addresses.

## Workflow for each function

1. Read `export/functions/<seg>_<off>_*.md` plus its callers and callees.
2. Check `notes/SYMBOLS.md` and `notes/CONVENTIONS.md`.
3. Write the Rust in `port/`, keeping names tied to the address until the purpose is understood (`fn fun_18cb_0944`).
4. Add or extend the trace test and run it.
5. Add what you learned (name, signature, struct fields, globals) to `notes/SYMBOLS.md`.
6. Commit.

## Order of work

1. Runtime and loaders: the message formatter (`%S`, `%W`, `%R`, `%N`), the resource loader (`Resource not found:`, `FindBlock`), and file access for `EPISODE?.DAT` and `game.cfg`.
2. DGROUP tables (characters, locations, items) extracted from the unpacked EXE.
3. Leaf functions, then upward, taking one segment (one source module) at a time.
4. Platform layer last: video, input, timer, sound behind clean traits.
5. Skip the Turbo C runtime and library code (startup, long-arithmetic helpers, printf family); replace with Rust equivalents.