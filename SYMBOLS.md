# Symbols

Address -> name, signature, purpose. Keep this sorted by address. Add a line when you learn something; mark confidence as `?` (guess), `~` (likely), `=` (verified by trace test).

Signature notation: `ret name(args)`, args in source order, `far` where it returns a far pointer.

## Functions

| Address | Name | Signature | Purpose | Conf |
|---|---|---|---|---|
| 1000:0000 | entry | `void entry()` | Turbo C startup, ends by calling main | ~ |
| 1000:03f1 | (hub, 39 callers) | ? | tiny helper, called from many places | ? |
| 1000:0396 | (hub, 29 callers) | ? | tiny helper | ? |
| 1772:0578 | (hub, 66 callers) | `?` | called first thing in many functions, probably a runtime/UI service | ? |
| 1772:000e | (hub, 52 callers) | ? | takes a message string, coordinates and flags | ? |
| 14b0:0bbf | (hub, 43 callers) | ? | 1221 bytes | ? |
| 15b8:0461 | (hub, 36 callers) | `far ptr f(...)` | returns a far pointer to a record, takes many args | ? |
| 15b8:16ca | (hub, 30 callers) | `far ptr f(a, b, c)` | called with 3 stack args from 18cb:0944 | ? |
| 2d65:08a4 | (hub, 31 callers) | ? | called before building messages | ? |
| 18cb:0944 | (unnamed) | `u16 f(u16 flags)` | moves the current entity by flags (0x1ff magnitude, 0x400 sign, 0x800 axis); returns 0 on arrival, 2 otherwise | ~ |
| 1000:054f | struct_copy | `void f(far src, far dst)` CX=len | Turbo C struct copy (`REP MOVSW` + `MOVSB`) | ~ |
| 2b79:0003 | render_room | `i16 f(u16 redraw_all)` | room renderer: seeds palette buffer `9BA0` from `2099`, draws scene objects (with per-id palette rules at `2b79:094f..0c42`) then placed characters; Ghidra splits it into `0397`/`0a7f`/`0aca` and leaves `0357..0396`, `03c6..07a0` unassigned. Palette path ported as `PaletteSet::scene_palette` (unit-tested against hand-derived vectors, not oracle) | ~ |

## Globals (DGROUP, segment 3364)

| Address | Type | Purpose | Conf |
|---|---|---|---|
| 3364:1f79..2399 | 11 x 96-byte tables | 32-entry 4-bit RGB palettes: `1f79`, `1fd9`, `2039`, `2099` room, `20f9` map, `2159` UI, `21b9`, `2219`, `2279`, `22d9`, `2339`. `2039` and `2099` differ in entries 11-13, 15, 17-31 | = |
| 3364:2399..23e7 | 3-byte colours | per-object palette patch colours used by `render_room` | = |
| 3364:8e5d | near ptr (seg `8e5f`) | active palette for the driver: `9ba0` buffer or a fixed table | ~ |
| 3364:9ba0 | 96 bytes | working palette buffer | ~ |
| 3364:9a6e | far ptr | current record/entity | ~ |
| 3364:9a1c | u16 | selector used by the 2d65:0ac8 switch (1-4) | ? |
| 3364:2c5e.. | records, stride 0x38 | character/entity table | ~ |
| 3364:40cf.. | records, stride 0x19 | locations | ~ |
| 3364:48a6.. | records, stride 0x1b | items | ~ |
| 3364:0389 | far ptr table, 16 entries | handlers in segment 18cb | ~ |
| 3364:79a6 | far ptr table, 3 entries | 1772:0578, 2920:1067 | ? |

## Structs

(none documented yet — add as you learn them, with offsets and sizes)
