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
