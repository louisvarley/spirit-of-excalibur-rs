# STATUS

Updated: M3 main-loop runtime port.

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
- Location travel list and save/load browser.
- End-to-end engine integration test (room, animation, save, font, story,
  travel).
- Main loop ported as `soe-engine::runtime::Runtime`: all screens are states,
  the original `-5`/`-4`/`-3`/`-2`/`-1` loop words are modelled, and
  room/map/status/story/travel/save/pause/action/item/target transitions and
  input routing live in the engine. `soe-app` drives `Runtime` and only
  presents frames/effects.
- State-transition unit tests plus an integration test over a real episode.

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

- Travel is list-based; graphical regional map/camera/current-location tracking
  remains.
- Events/NPCs/dialogue choices: story reader only; event tables not reversed.
- Inventory/economy/magic/trade: read-only status; item/stat layouts not
  reversed.
- Combat: not started; battle tables/formulas not reversed.
- Audio: SMUS parser + OPL2/cpal map music implemented; per-scene cues and
  driver-exact instruments remain.
- Packaging: release script and clean distribution folder implemented.
- Action menu and item-selection panel implemented (UI only).
- Battle unit record layout and battle UI code located; resolution pending.

## Next

1. Reverse event/NPC tables and dialogue-choice actions.
2. Reverse character/item/equipment layout and trade/spell actions.
3. Reverse battle setup/round tables and outcomes.
4. Per-scene audio cues, packaging polish, upscale.
