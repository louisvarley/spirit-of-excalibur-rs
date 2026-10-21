# Army battle

Status: **battle UI and round loop ported.** `soe-engine::battle` owns the
orders, round resolution, casualties and outcomes; `soe-engine::runtime` owns
the `Screen::Battle` transition and the return to the regional map.

## Captures and save states

The supplied screen captures show:

- Round Table units on the left, `Foes...` on the right.
- Defender/Challenger headings in the alternate (single-army) capture.
- Foe: `Melehan`, 100 knights, 825 soldiers, 250 archers.
- Order column: `Charge`, `Engage`, `Flank`, `Retreat`, `Do Round`, `Ignore`;
  `FUN_20b5_085b` also has an `Attack`/`Defend`/`Withdraw`/`Automatic` set when
  the caller's `0x10` flag is clear.
- Bottom message stream, e.g. `SIR BEDIVERE REELS FROM A SHARP BLOW.`
- A miniature battlefield between the text panes.

The battle save states (`start of battle`, `battle-ongoing-with-casulties`,
`battle-losses-are-heavy`, `battle-won-returned-to-map`) confirm the live lists
and the casualty fields. They are user-supplied references and are not
committed.

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

`Screen::Battle` is the headless equivalent. `soe-app` draws the same
Round Table/Foes columns, order column, strength totals, unit dots on the
miniature field and the last three messages.

## Round resolution (`FUN_20b5_1dff`)

`FUN_20b5_1dff` resolves one combatant at a time. The relevant path is the
attack case:

```text
skill   = max((unit[+0x2A] - 10) / 2, unit[+0x2D])
roll    = random(100)
defence = target[+0x2B] + target[+0x2C]
score   = skill - defence + order_adjustment
```

If the roll is below `score`, the target takes 1 hit point, or 2 when the roll
is below `score / 4`. A target whose hit points reach zero is removed from the
field (`+0x2C = 0`, clear presence bit `0x08`) and a routed/dead message is
appended. `FUN_2449_0faa` sums the current-strength words at `+0x28` for both
sides; the 2449 helpers also use it to compare army strength when a battle
ends.

The port keeps the same attack/defence/hit-point interpretation and adds the
casualties that the save files store: each hit subtracts
`(attacker strength / 20 + 1) * hit points` men from the target's `+0x28`.
`Charge`, `Flank` and `Defend` adjust the roll or incoming losses; `Retreat`
and `Withdraw` leave the field. Resolution uses a small seeded xorshift
generator so tests can reproduce every round.

Victory is reached when every foe unit is routed; defeat when every player
unit is routed; withdrawal ends the battle immediately. A 30-pass safety limit
awards the field to the side with the greater remaining strength.

## Runtime integration

- `Runtime::start_battle(&[player indices], &[foe indices])` builds armies from
  the live save state.
- `Runtime::start_battle_with_seed(...)` is the deterministic test entry.
- `Runtime::start_demo_battle()` uses the current save: present knights plus
  living army stacks, and `Melehan`/following stacks as the foe. This is the
  temporary encounter entry until the event/encounter table is reversed.
- `soe-play --battle` opens the demo battle.
- Up/Down (or click) selects an order; Enter resolves the selected order;
  Esc withdraws.
- On a terminal outcome, casualties are written back to the character records
  (`+0x28`, `+0x36`, death flag), `RuntimeEvent::BattleEnded` is emitted, and
  the runtime returns to `Screen::Map`. Saving afterwards persists the result
  through the normal save writer.

## Tests

- `battle::tests::round_damage_reduces_enemy_strength_and_hp`
- `battle::tests::battle_transitions_to_victory`
- `battle::tests::battle_transitions_to_defeat`
- `battle::tests::withdraw_ends_the_battle_without_round_damage`
- `battle::tests::casualties_are_written_back_to_character_records`
- `integration::battle_runs_multiple_rounds_and_returns_to_map_with_casualties`
- `integration::withdrawing_from_battle_returns_to_map`

Implemented model: `soe-engine::battle::{Battle, BattleUnit, Order,
BattleOutcome}`.

## Open approximations

- The exact AI target choice and per-unit pathing/formation (`2449:0aed`,
  `20b5:1705`) are presentation-heavy and not needed for the headless loop.
- Magic absorption, loot drops and the exact automatic-battle timing remain.
- Encounter setup still needs the event/encounter table; only the battle lists
  and record layout are verified.

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
