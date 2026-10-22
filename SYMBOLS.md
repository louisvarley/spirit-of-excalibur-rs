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
| 2b79:0003 | build_room | `u16 f(u16 arg)` | room/field scene builder (2b79:0003-0ea4; Ghidra splits it into `0397`/`0a7f`/`0aca`). Stamps stills into `DS:9680`, spawns scenery/animation (script 0x17), occluder (0x20), cut-out (0x10, id 0x3E7) and people (8) widgets, gates army soldiers 0x429..0x42F on `5E37`/`9C56`, then copies to `962C` and places the objects lying here (`4DD1`, filtered by `9DF5`/`9DAB`). arg != 0 fills with `9D1A`; 1 also presents. Returns 0xFFE8 if object 0x3E8 is present, else 0 (room base). Ported as `room_builder::fun_2b79_0003` | = (415 builds, `room_vectors.py`) |
| 2b79:0ea5 | choose_scene | `u8 f(u8 loc, u8 mode)` | `SCENES.RES` record `mode*100+loc`, or `DS:72EA[loc-0x3D]` for loc >= 0x3D; loads it into `72E6`, sets `9D18` count / `9D1A` colour, starts CHAPEL/CATHEDRL music for 0x38A..0x39F; returns 1 when found (callers store it in `9DC1`). Ported as `room_builder::{scene_record, fun_2b79_0ea5}` | ~ (unit test; record loading via the session) |
| 15b8:0978 | remove_widgets_by_depth | `void f(u8 lo, u8 hi)` | removes every widget whose depth `+0x0F` is in lo..=hi via `15b8:0834`. `ScriptScene::fun_15b8_0978` | = (via room builds) |

## Globals (DGROUP, segment 3364)

| Address | Type | Purpose | Conf |
|---|---|---|---|
| 3364:1f79..2399 | 11 x 96-byte tables | 32-entry 4-bit RGB palettes: `1f79`, `1fd9`, `2039`, `2099` room, `20f9` map, `2159` UI, `21b9`, `2219`, `2279`, `22d9`, `2339`. `2039` and `2099` differ in entries 11-13, 15, 17-31 | = |
| 3364:2399..23e7 | 3-byte colours | per-object palette patch colours used by `render_room` | = |
| 3364:8e5d | near ptr (seg `8e5f`) | active palette for the driver: `9ba0` buffer or a fixed table | ~ |
| 3364:9ba0 | 96 bytes | working palette buffer | ~ |
| 3364:9d18 / 9d1a | u8 / u8 | scene record object count / background colour (set by `2b79:0ea5`) | = |
| 3364:9d19, 9d10, 9d12 | u8, u16, u16 | depth and x/y where objects lying in the room go (set by objects 0x46F/0x472) | = |
| 3364:9dc1 | u8 | scene record found; the builder's `9DAB == 0` hooks set it to 0x63 | = |
| 3364:9dc2 | u8 | object 0x489 present | = |
| 3364:99bf | 4 x i16 | builder's current still rect (top, left, bottom, right), clipped to the animation union for cut-outs | = |
| 3364:4dd1 | 0x55 x 4 bytes | objects lying about: owner/item, location (`9DF5`), ?, flags (`&7` scene mode, `0x18`/`0xE0` hide) | ~ |
| 3364:79f7 | u8 per scene mode | floor line; room base is added to it | ~ |
| 3364:79f0, 7a05 | bytes | people: script per id; id per (count 0..2, scene mode), stride 7 | ~ |
| 3364:9a6e | far ptr | current record/entity | ~ |
| 3364:9a1c | u16 | selector used by the 2d65:0ac8 switch (1-4) | ? |
| 3364:2c5e.. | records, stride 0x38 | character/entity table | ~ |
| 3364:40cf.. | records, stride 0x19 | locations | ~ |
| 3364:48a6.. | records, stride 0x1b | items | ~ |
| 3364:0389 | far ptr table, 16 entries | handlers in segment 18cb | ~ |
| 3364:79a6 | far ptr table, 3 entries | 1772:0578, 2920:1067 | ? |

## Structs

(none documented yet — add as you learn them, with offsets and sizes)
