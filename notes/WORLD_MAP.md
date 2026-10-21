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
