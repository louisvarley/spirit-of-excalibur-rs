# STATUS

Updated: army battle loop (orders, rounds, casualties, return to map).

## Read this first

- `CLAUDE.md` ground rules and architecture invariants are authoritative.
- Oracle: `tests/oracle/excal_oracle.py` (Unicorn real mode). LZSS and RLE
  routines verified; vectors in git-ignored `tests/vectors/`.
- Prior repo hypotheses live in git-ignored `reference/prior/`.

## Done

- Workspace, `soe-core`, `soe-res`, `soe-assets`, `soe-video`, `soe-tools`,
  `soe-engine`.
- `.RES` header/type/directory schema verified; 841 records audited.
  LZSS 523/523 oracle-exact; flag-0x01 RLE layout verified.
- 5-plane image format verified (height,width,planes; even stride).
- Palettes: room `0x2099`, map `0x20F9`, UI `0x2159`, expansion `v*17`.
- `soe img dump` and `soe room compose`.
- Save/EPISODE loader and writer with the exact DGROUP mapping.
- SCEN schema and room compositor; all 138 scenes parse/compose; object flip
  confirmed.
- FRML fully decoded: 2-byte header, big-endian offsets, raw planar frames.
- `FUN_14b0_0451` formatter, oracle-verified for the text subset.
- Game font atlas located: `TITLE.RES#200` + EXCAL metrics; `BitmapFont`.
- Headless `Input`, `Timer`, `Sound`, `GameFiles` (read/write/list),
  `Platform`, `ResourceManager`, `Session`.
- Playable `soe-play`: winit window, 2x view, 18.2 Hz room animation,
  fade transitions, room/map/status/story/travel/save/pause screens,
  keyboard/mouse navigation, quick save/load.
- `HINT.RES` story reader (192 paragraphs) and original `INTF#621` panels.
- Graphical regional map: `INTF#1` 72x60 tile table, `INTF#500` tile atlas,
  `INTF#501` frame, location markers from the save table, camera tracking and
  location icons (`INTF#600..615`). See `notes/WORLD_MAP.md`.
- Location travel by keyboard/click with faithful location -> scene mapping
  (region scene, region-0 fallback, generic type scene), current-location
  tracking and map return.
- Save/load browser.
- End-to-end engine integration test (room, animation, save, font, story,
  map/travel, current location).
- Main loop ported as `soe-engine::runtime::Runtime`: all screens are states,
  the original `-5`/`-4`/`-3`/`-2`/`-1` loop words are modelled, and
  room/map/status/story/travel/save/pause/action/item/spell/shop/target/
  dialogue transitions and input routing live in the engine. `soe-app` drives
  `Runtime` and only presents frames/effects.
- Save decoding extended to the full 92 character records, 80 dialogue/event
  triggers, 49 object definitions and 85 live object instances.
- Inventory/economy/magic implementation: use, drop, pickup, give, equip,
  purse transfer, buy/sell trade, prepared-spell selection and spell effects.
  Status/items/spells/trade are live interactive screens.
- State-transition unit tests, economy tests for every action, interaction
  tests, plus integration tests over real saves/episodes.
- Army battle: verified 0x38-byte strength/HP fields, Round Table/Foes and
  Defender/Challenger UI state, six captured orders plus the alternate set,
  round damage/casualties, victory/defeat/withdrawal, write-back to the save
  records and return to the regional map. See `notes/COMBAT.md`.

## M1 remaining

- Animation timing/clock integration (frame cycling is implemented).
- MMAP records.
- START.EXE driver/load path.
- TITLE/LOGO palette code reference (room palette is visually confirmed for
  `TITLE#122`).
- Exact save-loader oracle diff needs DOS INT 21h emulation.

## M2 remaining

- OPL2/SMUS audio backend.
- Full DOS INT 21h file semantics.

## M3 remaining

- Driver callback layer inside the ported `FUN_2d65_2cd9` state machine.
- Text/UI rendering through `Video` (BitmapFont exists; driver-font path open).
- Palette animation and renderer culling details beyond the runtime fade.

## M4-M7

- Regional map/travel is graphical; real-time army movement, party sprites and
  the exact icon-variant choice remain open (see `notes/WORLD_MAP.md`).
- Events/NPCs: event table, Talk target and dialogue choices implemented.
  Per-scene trigger visibility and some event side effects remain open.
- Inventory/economy/magic/trade: implemented; exact price bargaining and a few
  object flag bits remain approximate.
- Combat: headless loop implemented. Encounter-table setup, exact AI target
  choice, magic absorption and loot drops remain approximate/open.
- Audio: SMUS parser + OPL2/cpal map music implemented; per-scene cues and
  driver-exact instruments remain.
- Packaging: release script and clean distribution folder implemented.
- Battle unit record layout, UI code and round/outcome loop are implemented;
  see `notes/COMBAT.md`.

## Next

1. Finish per-scene event triggers and remaining dialogue side effects.
2. Reach the battle from the event/encounter table (the loop itself is ported).
3. Per-scene audio cues, packaging polish, upscale.
