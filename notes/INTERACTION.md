# Event / NPC interaction

Status: **Talk, target selection, dialogue choices and the room action menu are
implemented** in `soe-engine::interaction` + `soe-engine::runtime`.

## Data

- Event triggers: file `0x1A4F` (`DS:46AB`), 80 records x 6 bytes.
  `{actor, relation, flags:u16, line:u16}`. `actor` and `line` are 1-based
  character and `HINT.RES` record indices.
- `flags & 0x70` is the priority used by `FUN_26c5_071a` when several records
  match the same actor; the lowest priority wins.
- `flags & 0x2000` marks a plea, `0x4000` a join-party offer.
- Character presence uses runtime `+0x16` (file `+0x20`) bit `0x08`.
- NPC templates live in the save character table at indices 65..92; the port
  parses all 92 character slots.

## Path

1. `Runtime::handle_click` hit-tests the room through `Session::object_at` and
   maps the object to a room character. `Runtime::select_character` opens the
   12-action menu.
2. Action `Talk` opens `Screen::Target` with the room list (excluding the
   actor). Confirm calls `Runtime::talk_to`.
3. `Dialogue::talk` selects the event record for the target, resolves its
   `line` from `HINT.RES`, and formats the speaker/listener header. The story
   reader walks `HINT.RES` directly because its record offsets differ from the
   generic `.RES` layout (raw payload at `offset+0x14`, LZSS at `offset+0x18`).
4. Join/plea records expose choices; applying a choice rewrites the event flags
   so state changes persist in the save file.
5. Give first selects a target, then opens the actor's carried item list and
   rewrites the instance owner (`FUN_1ba8_0431`). Pickup lists ground instances,
   Drop sets `{owner=0, scene=current, flags=0x04}`, Use applies the object
   definition effect.

## Regression fixtures

- `EXAMPLE.SAV`, scene 5: selecting the friar (0-based index 45) resolves event
  slot 18 to `HINT.RES` line 120 ("My lord Constantine, we welcome you...").
  The apothecary (index 43) owns carried objects used by the Give test.
- `D1.SAV` / `TRY1.SAV`: additional room saves used to check event/instance
  parsing and byte-exact round-trips.
- `save-state-2.sav` is a DOSBox-X capture supplied by the user and is not in
  the repository; the test is skipped when it is absent.
