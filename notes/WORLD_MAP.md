# Regional map and travel

Status: **graphical map/travel implemented** in `soe-engine::worldmap` and the
`Map`/`Travel` screens. The old text travel list is gone.

## Resources (`RES32/INTF.RES`)

| record | type | contents |
|---:|---|---|
| `1` | `MMAP` | 72 x 60 byte tile table (4,320 bytes) |
| `500` | `MMAP` | 256 x 256 five-plane tile atlas, 16 x 16 tiles |
| `501` | `MMAP` | 320 x 200 ornate frame, transparent centre |
| `600..615` | `MMAP` | location icons; the icon record is chosen by location `kind` |

`ENGLAND.RES#510` is a separate 320 x 200 overview map, not the scrolling map
used by this screen.

The map camera is 16-pixel tile based. `FUN_1851_06e1` stores the camera tile at
runtime struct `+0x40`/`+0x42`; the map is 72 x 60 tiles (1152 x 960 pixels) and
the viewport is 272 x 160 at logical `(16, 16)`. `FUN_20b5_2a30` centres a
location with a `-144, -80` bias, matching `FUN_2d65_0d39`'s marker draw.

Location icons are stored as two 48-pixel-wide variants side by side; the port
draws the first variant. A few `kind` values (`608`, `610`) are not image
records; those locations fall back to the ordinary village icon.

## Location records and travel

The save/EXE location table is 49 records of `0x19` bytes:

- `+0x00` name
- `+0x12` world X (map pixels, 0..1151)
- `+0x14` world Y (map pixels, 0..959)
- `+0x16` icon/type id (`600..614`; low 12 bits)
- `+0x18` flags

The live current location is `DS:9DF5` (1-based location number). Selecting a
location in `FUN_2d65_0bc8` sets it and calls `FUN_15b8_0c72`, which requests a
screen change through loop word `0`. The port stores the equivalent as the
0-based row in `Runtime::current_location`.

On startup/load, the current location is the nearest non-empty location to the
party leader's runtime world position (`character +0x2A` X, `+0x2C` Y). After
travel it is the destination location.

## Location -> scene mapping

`FUN_2b79_0ea5(location, region)` builds `region * 100 + location` for location
numbers below `0x3d`. If that record is absent, `FUN_2d65_13e2` retries with
region 0, which is the location number itself. This explains the captured
`DS:9DF5 = 19` room: scene `119` does not exist, so the loader falls back to
scene `19`.

Location numbers at or above `0x3d` use the DGROUP `0x72EA` table and the
generic scenes `901..928`. The port keeps the same order:

1. `region * 100 + (row + 1)`
2. `row + 1` (region 0)
3. generic scene from the icon type (`600..614 -> 901..909`)

Tests cover the mapping for every shipped location and verify each result is a
real `SCENES.RES` record.

## Input

- `T` opens travel; `M` opens the map. Both render the same regional map.
- Arrows cycle location markers and centre the camera on the selection.
- `Enter`/`Space` travels to the selected location (through loop word `-3`).
- Clicking a marker travels immediately.
- `T`, `M`, `Esc` close back to the room.
- The current location is outlined differently from the selection.

## Open questions

- Which icon variant (left/right 48-pixel frame) the original chooses for
  friendly/current/hostile locations.
- Whether the original map screen's palette is the room table for every screen
  or a per-object palette swap. The port uses the room palette, which best
  matches the captured frame.

## Travel step (`FUN_1b06_0007`) — ported, oracle-verified

Ported as `soe-engine::travel::fun_1b06_0007`. Verified against 10 915 real
calls recorded from the original in the emulator
(`tests/oracle/travel_vectors.py travel1:500 cont0:650 cont3:900`), including
waypoint picks and one arrival. The wake callback branch (`DS:0676`) has not
been exercised yet.

The UI framework calls it per map widget with the widget index; the entity is
`**(DS:969F + 4*index)`. Entity fields used:

| off | field |
|---:|---|
| `0x0A` | id passed to the wake callback |
| `0x0E` | moved this frame |
| `0x10`/`0x12` | y / x (pixels, map is `0x480` x `0x3C0`) |
| `0x1C`/`0x1E` | target y / x (`x == 0` means no target) |
| `0x20`/`0x22` | current waypoint y / x |
| `0x24` | carried movement budget |
| `0x26` | speed nibble (`<< DS:4FC4` per call) |
| `0x27` | followed entity id (looked up with `FUN_15b8_16ca(0, 0xD3, id)`) |
| `0x28..0x2D` / `0x2E..0x33` | last six waypoint tiles (y / x) |
| `0x34` | wake time, compared with `clock >> 8` (`DS:4FBF`, 32-bit) |

DGROUP tables: neighbour weights `0682`, neighbour offsets `068A`/`069A`,
probe->neighbour map `06AA` (low bytes of words), probe offsets `06CA`/`06EA`,
terrain speed `2A5C` (256 bytes), speed shift `4FC4`. Terrain comes from
`FUN_15b8_1b1b`: 16-pixel tiles of the 72-wide grid behind `[DS:8E29]`
(`+0x44` width, `+0x38` -> -> tiles), `0xFF` outside the map.

Step cost is `0x6400 / speed` straight and `0x7D00 / speed` diagonal (Ghidra
shows both constants as string addresses). Globals `DS:9B25`, `DS:9A7C`,
`DS:9A7E` are read before being written and so carry over between calls
(`TravelScratch`). Callbacks installed by `FUN_2d65_2cd9` via `FUN_15b8_0004`:
arrival `DS:0672 = 2920:026C`, wake `DS:0676 = 2920:06A3`.

On the original map only the party (widget 11) had a target during these
runs; clicking a location away from the party starts travel, clicking the
party's own location shows an info line ("King Clariance is at York").
