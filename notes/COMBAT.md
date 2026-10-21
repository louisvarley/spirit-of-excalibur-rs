# Army battle

Status: **army battles run on the ported engine** (`soe-engine::army`,
segment `2449`). `FUN_2449_134e` and its helpers are ported and verified
against the original; the headless round model that `battle.rs` used to
hold is gone (only `BattleOutcome` is left there).

## Live unit records

Battle units are the same 0x38-byte character/army records used by the save
loader. The battle lists at `DS:9DC2` (player) and `DS:9DD6` (foe) store
**1-based character indices**; battle record `n` starts at the name field of
character record `n - 1`.

| offset | field |
|---|---|
| `0x0A..0x15` | NUL-padded name |
| `0x20` | presence/status flags (bit `0x08` = in scene; death clears it) |
| `0x26` | maximum/starting strength (word) |
| `0x28` | current strength (word) |
| `0x2A` | primary battle image id |
| `0x2C` | secondary image id |
| `0x35` | battle defence field |
| `0x36` | battle hit points |
| `0x37` | battle attack skill |

The `0x2B..0x2D` values in `FUN_20b5_1dff` are name-relative, so they are
`0x35..0x37` on the save record. Examples from the capture:

```text
knight   strength 100/100  image 0x02A5  icon 0x02BC
soldier  strength 825/825  image 0x02A6  icon 0x02BC
archer   strength 250/250  image 0x02A6  icon 0x02C3
```

The battle-ongoing save shows the middle column as casualties:

```text
knight  60 -> 18
knight  50 -> 12
soldier 300 -> 0
archer   50 -> 0
soldier 825 -> 568
```

## Battle UI (`FUN_20b5_085b`)

`FUN_20b5_085b`:

- Reads the player/foe lists and counts at `DS:9DAC` / `DS:9DAD`.
- Draws `ROUND TABLE...` / `FOES...`, or `DEFENDER...` / `CHALLENGER...`
  when bit `0x10` is set.
- Renders each unit's name or strength/type row and the primary/secondary
  image ids.
- Chooses the order menu from the same bit: the captured six-order menu or the
  `Attack`/`Defend`/`Withdraw`/`Automatic` set.
- Writes the rolling message stream at the bottom and the miniature battlefield
  in the middle.

`soe-app` draws the same Round Table/Foes columns and order column over the
stone panel, the field and its tokens (`Session::compose_army`) and the
latest message.

## How the original runs an army battle (2449, ported 2026-10-22)

`FUN_20b5_2a30(mode)` with bit 0x10 calls `FUN_2449_134e`, which runs the
whole battle inside the UI loop's waits (`FUN_15b8_09f7(&flag)` runs the
scene until the DGROUP word is 0):

- Set-up: `DS:03C5` (script handler 15) becomes `2449:12DE`; `FUN_2449_0aed`
  places the tokens; with a screen (`9C56 & 0x20` clear) the panel
  `FUN_20b5_085b` is drawn and the tick `2449:0A3C` is installed in
  `DS:01F6`, which clears the message wait `DS:9C58` every 0x78 clock ticks.
  Each side's leader (`9DAE` / `9DB0`) standing at a castle (location record
  word `+0x16` 0x258, 0x259, 0x25C or 0x25F) is noted; the deltas between the
  two leaders are how far a routed side moves on the map. Starting totals go
  to `DS:9CC6` / `9CC8`.
- Order loop (`15CB`): `FUN_2449_0255`, wait `9C58`, maybe wait the order
  menu `9CB0`; a round is fought once `DS:9CDA` is set: by `0255` when the
  fronts meet, by a token's script (handler 15 op 9), or at once off screen.
- Round (`1653`..`15CB`/`26D8`), see `fun_2449_134e_round`:
  side one's orders change state (0x80 -> 1, 0x40 -> 0x80, charge 2 -> 0x40,
  flank 3 -> 5 -> 1); power = sum of strength x attack `+0x2D` (16-bit
  product, sign-extended; +2 x attack when `+0x1C` == 1; a charge 1.5 x
  strength, a flank's second round double). From the second round (or with
  a side empty) `FUN_2449_1021` may end the battle. Damage to a side =
  `(rand(8) + 12) x enemy power / own strength` (32-bit, unsigned divide).
  A character takes a wound per 0x80 of damage whose `rand(0x80)` is below
  what is left and whose `rand(100)` beats `+0x2E`; at 0 hit points they are
  killed and `FUN_20b5_1229` runs (a raised story event ends the battle).
  An army stack loses `strength x damage / (defence +0x2B + castle) >> 7`
  men (1.5 x in state 0x80); losing all of them clears it from the field
  and the map. Off screen nothing is shown and token images stay.
- `FUN_2449_1021(s1, s2, castle1, castle2)`: 3/4 a side is gone; nothing
  while one side is three times the other; else a side breaks when its
  share `(s << 4) / start` is below 8 (14 at a castle) or the other share
  exceeds it by more than 3 (2). Side one at its castle is asked "Fall
  back to the walls?" (`FUN_2449_0107`). Ghidra misreads several of these
  branches; the port follows the disassembly.
- After a rout (1, 2) units that do not hold a castle move away on the map
  and get a cooldown `+0x1A = (clock >> 8) + 0x3C` (0x46 inside a castle),
  then `FUN_1b06_071c`. The end (`26D8`) waits three seconds (`DS:0322`),
  frees the token images and restores handler 15.

Tokens (`FUN_2449_0aed`): flags 0x248 (+1 for side two), depth 0xF5, id
`character << 8 | slot`, image `DS:9CCA[k]` = resources 0x2EE + k by kind and
strength, script 0x0D characters, 0x0E soldiers, 0x0F archers, 0x10
knights (+4 for foes), place from `DS:64B8` (6-byte x, y, taken, first
step), starting at the place's step + 1. Knights fill places from the back
(`0x12` down to `0x0C`); Ghidra gets that loop's bound wrong. Tokens are drawn
at absolute screen coordinates over the field image 0x26C (0x26D for terrain
classes 1, 6, 9, 12) at (0x90, 0x50). The battle keeps the map palette.

Order buttons: Charge `2449:0420`, Engage `04F2`, Flank `056B`, Retreat
`063A`, Do Round `0764`, Ignore `0ADE` (sets 0x20: fought off screen);
a unit row or token click is `0792`, availability `DS:5E30[order] >> kind`.

### Verification

`tests/oracle/army_watch.py` starts a battle in the original from the map
snapshot (sets the side lists and calls `FUN_20b5_2a30(0x10)` from the map
loop; the RES32 directory must be present). `tests/oracle/army_vectors.py`
records five battles (on screen and off, extra orders given) and
`army::tests::matches_original_army_calls` replays all 74 calls: 13 rounds
(charge, flank, kills, a story-event death, rout ends), 8 rout checks, 13
targeting passes, 5 token builds, 9 totals and 26 handler-15 calls. Not
covered: the "fall back" answer, retreats, defending (`9C56 & 8`) and castle
battles. A story event (`FUN_26c5_000b`, unported) changes character records,
so those are not compared after one.

### In the Rust build

`ArmyBattle` (`army/battle.rs`) keeps `134e`'s waits as phases and runs one
scene frame per 60 Hz tick, so tokens walk by their scripts and handler 15
calls `FUN_2449_12de`. Approximations: a round body runs in one go and its
message waits are then replayed two seconds apart (strength changes appear
when the round starts); the random generator is a stand-in; the terrain
under the leader is not passed in yet (field 0x26C); the "fall back"
question answers yes; the order menu's "Select an option" step and the
panel's mark sprites are not drawn. `Runtime::start_battle` and the map's
army confrontations open it; `soe-play --battle` opens the demo battle.
Up/Down pick an order, Left/Right a unit, Enter gives it, Esc ignores.

## Single combat (observed in the original, 2026-10-21)

`tests/oracle/duel_watch.py map 1 62` makes the party walk past Lupinus (with
B's entity given movement class 2 so the 1-pixel class-0 radius is not
needed). The fight check returns mode 1 ("Sighting ..., ... prepares for
single combat"), `FUN_20b5_2a30(1)` switches to the field scene, Lupinus
speaks, and the duel then runs on the **same battle screen** as army
battles: "Round Table..." (Constantine) / "Foes..." (Lupinus), orders
Attack / Defend / Use Item / Magic / Withdraw / Automatic, round messages
such as "Sir Constantine fails to parry. He is wounded."
Snapshots: `tests/vectors/emu/duel.snap` (speech) and `duel2.snap` (battle).

So single combat is the battle engine with one unit per side. The runtime
now does the same. The round engine itself, `FUN_20b5_1dff`, is a per-unit
script interpreter (9 opcodes via the `CS:1EC6` table) driven by timers and
sprite state, run by the `20b5` scene functions on the `15b8` widget loop;
`battle.rs` is still a headless approximation of it and is the next thing
to port faithfully, using these snapshots as the oracle.

Meeting rules worth knowing when setting up tests: encounters are ignored
before clock `0xA00`; two characters with no `0x700` flags only meet when
one is kind 4; the collision radius is `DS:067E[max(class)]` = 1, 8, 16, 32
for movement class `+0x26 >> 4 & 3`.

## `FUN_20b5_1dff` ported (`soe-engine::combat`)

Opcode = high byte (CS:1EC6 table): 0 order wait, 1 random check, 2 random
timer, 3 fixed timer + attitude, 4 morale, 5 flee/withdraw, 6 attack,
7 battle start (scene set-up, returned as an event), 8 magic (absorption).

Attack (op 6): chance = (operand & 0x7F) - target `+0x2E`, plus
attack - target `+0x2D` for weapon attacks (attack = max(`+0x2D`,
(`+0x2A` - 10) >> 1)); `rand(100)` below chance hits, below chance/4 hits
for 2; a miss within twice the chance is a parry (state 8). HP is
`name+0x2C`. In a contest (mode bit 4) the first wound concedes; otherwise
HP reaching 0 kills (flag 8 cleared, "%S is dead."). Wound messages come
from the far-pointer table `DS:5DCA` (`rand(4) + 4*damage`, magic
`damage + 12`).

Original bugs reproduced: op 6 reads the uninitialised `[BP-0x10]` in the
contest path; op 5 calls `FUN_26c5_041b` with two arguments, so its delta
is the previous call's leftover (-2 or 0).

Verified on 464 recorded calls (`tests/oracle/battle_vectors.py duel2 2000
8`): opcodes 0-4 and 6, including a kill. The eight runs replay the same
duel (the delays did not change the random draws), and opcodes 5 and 8
are not covered yet. Note for recorders: same-segment calls here are
`PUSH CS; CALL near`, so arguments start at SP+4.

`battle.rs` still runs its own round model; switching it to drive these
steps needs the per-unit script interpreter around them (the `20b5`
scene functions), which is the next piece.

## Where the duel is animated (traced 2026-10-21)

The duel's fight is animated by the field scene. The battle screen only shows
the menus. Logging `FUN_15b8_0461` calls while `duel_watch.py map 1 62` runs
shows the animated widgets (flag 0x200) being created by:

- `2b79:036c`: scenery (flags 0xC240, script 23 (0x17), depth 4).
- `2d65:18a8` inside `FUN_2d65_13e2`: Lupinus, id 0x3E, `+0C` 0x19, flags
  0x241 (mirrored), script 0, depth 184 (0xB8), entering from x 250.
- `2d65:1ae7` inside `FUN_2d65_13e2`: Constantine, id 1, `+0C` 1, flags 0x240,
  script 0, depth 184 (0xB8), entering from x -30.

Each fighter's `+2E` names the other. Script 0 (`DS:070C`, static EXE data,
like scripts 0-23 at `DS:1E2A`) walks them together and calls handler 15 =
`FUN_20b5_1dff` for each exchange. `FUN_18cb_0002` is the scene loop: it
runs `FUN_18cb_066a` for every 0x240 widget whose timer is due, then
composites the redraw rects.

`FUN_2449_0aed` is different: it builds the army-battle tokens. Its flags are
0x248 or 0x249, its scripts 0x0D-0x10 (+4 for foes), its images resources
0x2EE-0x2F1, and its slots come from the table at `DS:64B8`.

So, to run the duel on the real engine, the port needs `FUN_2d65_13e2` (field
scene set-up), `FUN_18cb_0002` (scene loop) and a combat `ScriptHost::hook`
calling `combat::fun_20b5_1dff`. The interpreter itself is already ported
(`script::ScriptScene`).

## Duel on the real engine (ported 2026-10-21)

- `FUN_18cb_0002` (scene frame), `FUN_18cb_05b0` (followers),
  `FUN_15b8_0834` (removal) and `FUN_15b8_0461` (widget creation, all three
  paths) live in `script::ScriptScene`. The frame returns the dirty rects
  the original redraws; the drawing itself is for the presenter.
  Verified: 4,498 frames (`scene_vectors.py --frames`).
- The character half of `FUN_2d65_13e2` is
  `ScriptScene::fun_2d65_13e2_characters`. Foes (`DS:9DC3+0x14`) spawn
  first, then the party. y = room base + `DS:7A2E[slot]` + `DS:7A66[kind]`;
  x comes from `DS:7A1A[slot]`, or from `rand(foes*10) + DS:7A38[kind]`
  depending on the mode bits (`9DAB & 0xC0`). pc is 3 for the side's lead
  and 4 for the rest; `+34` is the list slot. Verified on the two duel
  fighters (`tests/oracle/duel_spawn_vectors.py`; mode 0 only). The room
  builder `FUN_2b79_0003`, banners and music are not ported here.
- Handler 15 is `combat::script_hook`. The target is
  `ScriptScene::find(0x200, 0, actor+2E)`. Verified: all 65 duel steps,
  including 57 combat calls. The check covers the widgets, the sounds and the
  DGROUP regions the combat step owns. `DS:01FC` is the random seed, and
  `FUN_15b8_17e0` advances it.
- Note for recorders: the stack segment lies inside DGROUP's 64 KB
  (SS:SP = DS:9E90+SP in the duel), so DGROUP diffs must skip the stack.

## Duel in the Rust build (2026-10-21)

Single combat opens `Screen::Duel` (`soe-engine::duel`, `soe-play --duel A B`).
The duel runs on the ported scene engine over the live map's DGROUP. Its
backdrop is the field scene (`FUN_2b79_0ea5`: field types 0x3D and up map
through `DS:72EA`, so type 73 gives `SCENES.RES` 907). Each champion is
drawn as an `ANI.RES` frame at its widget position. When the duel ends, any
key copies the character records back to the map.

- Opcode 7 of `FUN_20b5_1dff` (`20b5:2769`, battle start) is now ported in
  `combat::script_hook`, because it pairs the champions inside the scene: it
  sets the slots, script step 6 and timer 0, and points each `+2E` at the
  other. The pairing was found by watching writes to `+2E` from `duel.snap`.
- Approximations, all noted in `duel.rs`:
  - Field type 73 is fixed; the terrain rule of `FUN_2d65_13e2` isn't ported.
  - The room builder isn't ported, so the field scene's army soldiers are drawn.
  - No colour remap from widget `+0C` (Lupinus should be black).
  - No rout check.
  - Message names are filled in by rule.
  - The duel ends on the death, concede, withdraw or flee message, because
    `FUN_20b5_1057` and unit removal aren't ported.
  - The host clock advances one tick per 4,096 reads within a frame, so
    `066a`'s deadline still trips.
  - The random generator is a stand-in.

## The loser and the colours (ported 2026-10-21)

- `FUN_20b5_1705(widget, flag)` handles a unit leaving the fight. It looks for a
  new opponent on the other side (waiting units first). If anyone is still
  fighting, it waits. Otherwise it ends the battle:
  - sets `DS:01EE`, sets `DS:9CAC`, and puts the close time at `DS:0322`
    (clock + 0xB4, about 3 s);
  - plays the music (`DS:6168` or `BTLLOST` at `DS:616F`);
  - settles each unit by its slot state through the `CS:1A03` table:
    fighters to step 0x14 or 0, the dead through `FUN_20b5_1229`, and for the
    wounded and the fled, stat changes and map moves (`1b06_06b5`/`071c`).
- `FUN_20b5_1229` is the dead character's bookkeeping. Followers pass to an
  heir, the position goes to the top leader, flag 8 and strength are cleared,
  and the first matching death event in `DS:46AB` is raised
  (`FUN_26c5_000b`).
- Both are in `combat.rs`; `script_hook` runs `1705` after a step that emits
  Rout-on-actor. Verified on the recorded end of the duel
  (`tests/oracle/rout_vectors.py`; one call, flag 1). Not yet covered by any
  recording: the new-opponent search, the wait, and slot states 8, 0x10 and
  0x20. Opcode 5's target rout check is still a stand-in.
- `FUN_20b5_1057` drops the dead unit's items when `DS:9DF6` is set. Each
  qualifying item has a 40% chance; it leaves the inventory (`1ba8_01ff`,
  returned as an event) and is spawned as a still image near the body.
  Ported from the disassembly. **Unverified**: no recorded loser carried
  anything. The duel test gives both champions an item to exercise the path.
- Colour remap: the scene draw passes widget `+0C` to `FUN_14b0_0bbf`, which
  gives MCGA.DVR the 32-byte table at the far pointer `DS:29A4[+0C]`.
  `drv_0` flag 2 then draws each non-zero pixel `p` as `table[p]`; `+0C` = 0
  means no remap. Lupinus (`+0C` 0x19) is drawn black, as in the original.
- In the Rust build the duel ends as the original does: once `1705` sets
  `DS:01EE` and the close time passes. By then the loser has collapsed
  (step 0x0C) and the winner has walked off (step 0x14).
