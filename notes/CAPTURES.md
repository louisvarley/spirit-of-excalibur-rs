# Original captures and save state

User-provided references (do not commit the binaries/screenshots).

## `screenshots/excal_001..010.png`

640x400 (2x logical), captured from the original:

- `001/002`: regional map, `ENGLAND` frame, compass/hourglass/etc. icons.
- `003`: map event menu with two choices.
- `004`: dialogue over a room (blue knight + townsman).
- `005`: same room without the dialogue panel.
- `006/007`: battle screen with Round Table/Foes columns.
- `008`: post-battle dialogue over the regional map.
- `009`: save/load menu over the regional map.
- `010`, `stats-on-party.png`: army/status table over the map. Columns are
  `NAME hp hth MV [weapon] COM ARM NOB FTH`; equipment is supplied by carried
  object instances, not by extra character fields.
- `options in room.png`: the 12-action room menu (Pickup, Drop, Talk, Use,
  Search, Trade, Request, Magic, Seize, Give, Bribe, Attack).
- `new_excal_003.png`: the Give item list (`WHAT SHALL CONSTANTINE GIVE TO SIR
  BELLENGERUS?`), showing one carried item per line.
- `select.png` / `magic no magic available.png`: target selection and the
  original "has no magic prepared" message.

These confirm:

- Text uses the `TITLE.RES#200` atlas with the UI palette.
- The ornate map frame is `INTF.RES#501` (320x200, centre transparent).
- Map screens combine a regional map, the frame, icons and text.
- The scrolling background is the `INTF.RES#1` 72x60 tile table drawn with the
  `INTF.RES#500` 16x16 tile atlas; location icons are `INTF#600..615`.
  See `notes/WORLD_MAP.md`.
- Dialogue/menu panels are drawn over the current room/map.

## `save-state.sav`

DOSBox-X save state, a ZIP archive. Key entries:

| entry | contents |
|---|---|
| `Memory` | guest RAM plus an 8-byte dump header |
| `Vga` | VGA state/framebuffer |
| `CPU`, `Dos`, `Keyboard`, `Mouse`, ... | machine state |

Practical recovery from the memory dump:

- DGROUP is found by searching for `Turbo-C - Copyright (c) 1988 Borland`.
  In the provided state it sits at guest linear `0x2B8F0`.
- Live font metrics at DGROUP `0x1E8A..0x1E8C` match the EXCAL file.
- The decoded font atlas is in guest memory as a linear 8-bit buffer with a
  **304-byte row stride** (301 pixels + 3 bytes padding), not as a 5-plane
  record.
- The live scene-object pointer is at DGROUP `0x9D14`, count at `0x9D18`;
  objects use the same bitfields as `SCENES.RES`.

The first save state was captured on the regional map, so no ANI room animation
data was resident.

## `save-state-2.sav` (room scene)

- Same DGROUP base (`0x2B8F0`) and font metrics.
- Current scene id is **19** (`DS:9DF5 = 0x13`).
- The live object list matches `SCENES.RES#19` exactly: 15 objects, first
  `1022@0,0`, then `1403@0,100`, `1050@0,26`, ...
- The list was located at dump offset `0x4D530`; `DS:9D14` in this frame points
  at a different render buffer, so use the `SCENES.RES` sequence to locate the
  live copy.
- Scene 19 has no `700..799` or `900..999` objects, so no ANI frame bank was
  resident.

## `save-state-2.sav` (NPC dialogue)

A later capture was taken with an NPC dialogue open in scene 5.

- Rendered dialogue buffer at `0x34F23`:
  `"These are strange times we live in."  rec b`
- A source/runtime copy exists at `0x950E2`, followed by a small node table.
- Live NPC character records are in memory around `0x2E800`:
  `Palomides`, `Hebes`, `Lionel`, ... with the same 0x38-byte character shape
  as the save file.
- The per-scene trigger table is now mapped: `EPISODE*.DAT`/`*.SAV` file
  `0x1A4F` holds 80 six-byte `{actor, relation, flags, line}` records, and
  `line` is a 1-based `HINT.RES` record id. `soe-engine::interaction` resolves
  Talk through those records. The saved `save-state-2.sav` capture itself is
  user-supplied and not committed; regression fixtures use `EXAMPLE.SAV` and
  the provided `D1.SAV`/`TRY1.SAV` room saves.

FRML was already solved from the earlier room capture, so no further animation
state is needed.
