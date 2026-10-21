# SCENES.RES room records

Status: **CONFIRMED** for scene 1 (and consistent with the prior notes).

Container: normal `.RES`, type `SCEN` (`NECS` reversed), 5-dword header,
payload base `0x14`, 138 records, flags `0x00`.

Record layout:

```
u8  object_count
u8  background
object[object_count] * u32
u32 trailer          (optional; some records end after the object list)
```

All 138 records parse; scene 1 has the trailer, others (e.g. a 20-object,
82-byte record) end at the object list.

Object bits (little-endian u32):

| bits | field |
|---|---|
| 0..10 | resource id |
| 11..19 | stored x + 190 |
| 20..27 | y |
| 28..31 | flags (bit 0 = horizontal flip) |

Draw order is file order; index 0 is transparent. `FUN_2b79_0003` passes
`flags & 1` to the blitter, and `FUN_14b0_0bbf` folds it into bit 7 of the
sprite control word (`control ^= (flags & 1) << 7`), i.e. horizontal mirroring.
Bits 1..3 are not consumed by the renderer. Resource ids in the ANI
ranges (`0x2BC..0x320`, `0x384..0x3E8`) are FRML animations, all others IMAG
images. The background byte is informational; the object list alone composes
the room.

Verified scene 1: 16 objects, background 14, trailer `0x2A`; first objects
`1039@0,0`, `1403@0,100`, `1041@183,32`, `1041@-136,31`, `1041@0,40`,
`1407@135,32`; object 15 is animation `908`. `soe-engine::scene::Scene` parses
this and the test checks the vector.

Still unverified: the exact renderer routine address and the MMAP metadata
records (7 in INTF.RES).

## Widget script interpreter (segment 18cb), ported in `soe-engine/src/script.rs`

- `FUN_18cb_066a` runs the current widget (`DS:9A6C`) for at most 60 clock
  ticks. It saves the old rect to `+0x1C`, then dispatches `word >> 12` through
  `DS:0389` until a handler returns 1 (wait) or 6 (show frame). On 6 it moves
  the widget by the difference between the old and new frame anchors (sheet
  frame records: 6 bytes from header byte 4; `[2]` anchor x, `[3] & 0x7F`
  anchor y, `[4]` w, `[5]` h). It then unions the old and new rects
  (`FUN_18cb_16f0`), clips the result to `DS:03C9` (`FUN_30ec_0006`) and sets
  the redraw flag if any of it is visible.
  Verified: 302 steps (`tests/oracle/scene_vectors.py`, duel2/hall2/s4).
- Handler 11 (`15d3`) plays sound `arg`. An earlier note said it used a stale
  stack word; that was wrong.
- `FUN_15b8_162e(mask, depth, id)`: the first widget in the list whose `+0A`
  equals `id`, with depth `+0F` (0 = any) and a flag in `mask` (0 = any).
- Handlers 5 (show or hide the partner), 12 (spawn, end, end copies, hide,
  stamp) and 13 (image, id, `+0C`, depth reorder) are ported from the
  disassembly. None of the recorded snapshots calls them, so they are
  **unverified**. The same applies to the spawn path of `FUN_15b8_0461` (the
  parent-copy case only) and `FUN_335c_000f`, which is `toupper`.
