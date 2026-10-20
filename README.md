# Spirit of Excalibur — Rust reimplementation

A faithful Rust reimplementation of *Spirit of Excalibur* (Synergistic /
Virgin-Mastertronic, 1990, DOS). This repository contains **only our own code
and notes**. It does not contain the original game.

## You must supply your own game files

Put your legally owned copy of the DOS game in `game/` (or set `SOE_GAME_DIR`).
The tests that need original data are skipped when the files are absent. Never
commit game files, unpacked executables, converted assets, disassembly dumps or
trace vectors.

## Workspace

| crate | purpose |
|---|---|
| `soe-core` | explicit 16-bit helpers, DX:AX packing, far pointers |
| `soe-res` | `.RES` container reader and decompressor trait |
| `soe-assets` | resource IDs, layered asset sources, override packs |
| `soe-video` | `Video`, `Upscaler`, indexed reference renderer, scaled presenter |
| `soe-tools` | developer CLI (`soe res info`, image/room/font dump) |
| `soe-engine` | resource, save, scene, room, font, formatter and session logic |
| `soe-app` | playable `soe-play` window (rooms, animation, map, status) |

## Design constraints

- Game logic uses logical 320x200 coordinates only.
- Images are addressed by resource ID, never by raw buffers.
- Original images stay indexed; override packs are RGBA.
- `Video` is the platform boundary; the reference renderer is pixel-exact.
- 16-bit semantics are explicit (wrapping, signed/unsigned compares, DX:AX).

See `notes/ARCHITECTURE.md` for rationale and open questions.

## Build and test

```sh
cargo test
cargo run -p soe-app --release
cargo run -p soe-tools -- res info game/HINT.RES
```

`soe-play` controls: arrow keys or click to change room/page, `M` map,
`I` status, `H` story chronicle, `P` pause, `Space` step animation, `R` reset,
`Esc` quit. See `notes/PLAY.md`.

Tests that read original files use `SOE_GAME_DIR` (default `game/`). They print
a skip message and pass when the files are absent.

## License

MIT OR Apache-2.0. The original game remains the property of its rights holders.
