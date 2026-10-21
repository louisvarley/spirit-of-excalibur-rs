# Segment 1ba8 — room actions, inventory and item/spell effects

Survey of `CODE_13` (`1ba8:000f`-`1ba8:48e8`, ~18.6 KB, 50 exported
functions). Confirmed: this is the module behind the room **action menu**
(`FUN_1ba8_4604`: Pickup, Request, Trade, Seize, Attack, Set down, Give,
Bribe, Talk, Search, Use, Magic) plus the inventory primitives and the
item/spell effect dispatcher. Functions with no callers are menu handlers
reached through the menu, not dead code.

Purposes below come from the disassembly, the strings each function loads and
its callers. "Ported" means verified under the oracle; everything else is a
reading, not a verified fact.

## Data it works on

| Address | Meaning |
|---|---|
| `DS:2C5E + id*0x38` | character record; `+0x26` kind (`0x16` statue), `+0x28` gold word, `+0x16` presence flags (`& 0x8208 == 0x208` checked by 2611) |
| `DS:4DD1 + i*4`, `i < 0x55` | item instance `[def, owner, count, flags]`; `flags & 0x18 == 0x18` carried by character `owner`, else ground item in scene `owner` at spot `flags & 7`; `flags & 1` equipped/prepared bit (2ad2) |
| `DS:488B + def*0x1B` | item definition; name at `+0`, `+0x18` word (`>> 12` = class), `+0x17` effect id (3032 switch) |
| `DS:9C00` | byte set by 0058 |
| `DS:9C01`, `DS:9C02` (far ptr) | 001c: buffer from `FUN_15b8_16ca(0x200, 0, [9C01])` |
| `DS:9C06` | definition of the last item moved (0431, 01ff) |
| `DS:9A79` | set to 1 by 001c (redraw request?) |
| `DS:9DF5` | current scene (owner of ground items) |
| `DS:9DAB` | spot where dropped items land |
| `DS:5040`, `DS:504D`, `DS:500C` | per-stat tables used by 2611 (random bound, threshold, label far ptr), indexed by `stat - 0x24` |
| `DS:1E8A` | list row height used by 078f |

## Functions

| Function | Size | Purpose | Status |
|---|---|---|---|
| `001c` | 60 | allocate the 0x200 work buffer, error 0x35 on failure, set `9A79` | |
| `0058` | 16 | UI reset then `9C00 = arg` | |
| `0068` | 69 | statue check: message "You can't do that with a statue!" | **ported** |
| `00ad` | 338 | take an item (count prompt "How many of the %d %ss shall %S take"; eldritch blade special) | |
| `01ff` | 562 | character drops an item to the ground (gold: random amount, merges piles); used by battle drops `FUN_20b5_1057` | **ported** |
| `0431` | 204 | transfer `count` of a definition between owners (gold: purse words) | **ported** |
| `04fd` | 130 | count carried instances of a definition (gold: purse) | **ported** |
| `057f`/`05b3` | 50+476 | list builder used by Pickup and `FUN_26c5_111d`; Ghidra split it at a switch (`caseD_3f`) | |
| `078f` | 578 | inventory list builder: groups carried instances by definition, filters by definition flag word `+0x18`, emits rows via `FUN_15b8_0461`; returns next y | |
| `09d1` | 49 | helper for Pickup | |
| `0a42` | 88 | Pickup confirm | |
| `0a9a` | 193 | **Pickup** ("What item shall %S pick up?") | |
| `0b5b` | 547 | **Request** outcome (gives / hints / church / refuses / scowls) | |
| `0d7e` | 201 | Request: item choice ("What item shall %S request from %S?") | |
| `0e47` | 47 | Request: target choice ("Request of whom?") | |
| `0e76` | 399 | item effect on pickup/trade (eldritch blade); uses `14b0`/`17f3` | |
| `1005` | 1771 | **Trade** dialogue (Offer More / Request Less / End Trade) | |
| `16f2` | 104 | Trade: target choice | |
| `175a` | 587 | room character list for target menus (with `FUN_2a6e_01e7`) | approximated in `interaction.rs` |
| `19a5` | 400 | **Seize** item choice | |
| `1b35` | 46 | Seize: target choice | |
| `1b63` | 300 | **Attack** ("Attack whom?"), stat change via 2611, then 175a | |
| `1c8f` | 353 | Set down worker (calls 01ff) | |
| `1df0` | 203 | **Set down** ("What item shall %S set down?") | |
| `1ebb` | 656 | **Bribe** outcome | |
| `214b` | 201 | **Give** item choice | |
| `2214` | 47 | Give: target choice | |
| `2243` | 172 | **Bribe** target ("Bribe whom?", "%S's purse is empty") | |
| `22ef` | 102 | **Talk** target → `FUN_26c5_071a` | approximated in `interaction.rs` |
| `2355` | 377 | **Search** body (gold / item / nothing) | |
| `24ce` | 314 | Search worker | |
| `2611` | 447 | stat change `(stat, id, delta)` with "%S %s %d %s point(s)" and the reproach death; called by battle code | recorded as a subcall in `combat.rs` |
| `27d0` | 770 | apply an effect to a stat ("%S feels no change") | |
| `2ad2` | 213 | find a carried instance by definition / bit 0 / class | **ported** |
| `2ba7` | 104 | spell target choice ("Select a target", "No one here is dead", not in battle) | |
| `2c0f` | 395 | effect helper (`14b0` graphics) | |
| `2d9a` | 102 | "Powerful defensive magic absorbs…" | |
| `2e00` | 39 | "Nothing happens." | |
| `2e27` | 521 | Use/Cast entry: picks target, calls 3032 | |
| `3032` | 4378 | effect dispatcher: 66-case switch on definition `+0x17` (`1ba8:324e`) | |
| `3a0b` | 27 | switch case 0 → 421a + 27d0 | |
| `421a` | 446 | "%S %S vanishes in a puff of smoke" | |
| `43d8`/`3f68` | 4 | empty stub and its thunk | |
| `43dc` | 277 | **Use** item choice | approximated by `economy.rs::use_item` |
| `44f1` | 272 | **Magic** spell choice → `FUN_2b79_0a7f` | approximated by `economy.rs::cast_spell_slot` |
| `4604` | 741 | room action menu ("Select an action for %S") | layout mirrored in `window.rs`, `soe-app` |
| `05b1`, `25c4` | 1 | alignment bytes, not functions | |

## Approximations still in `soe-engine`

- `economy.rs` (`GameState` methods: `give_item`, `drop_item`, `pickup_item`,
  `buy`, `sell`, `use_item`, `cast_spell*`, `inventory`) re-implements 0431,
  01ff, 04fd, 078f, 1005, 1df0, 2355, 43dc, 44f1 on the parsed save rather than
  on DGROUP. The ported `inventory::fun_1ba8_*` work on a DGROUP image (like
  `names.rs` and `combat.rs`); swapping them in needs `runtime.rs` to hold a
  DGROUP, which the other sessions' merge will decide. Not swapped yet.
- `interaction.rs` approximates 175a and 22ef.
- `combat.rs` reports 2611 and 01ff as `CombatEvent`s; `fun_1ba8_01ff` can now
  serve the `ItemDropped` event directly.

## Verification

`tests/oracle/inventory_vectors.py` calls the ported functions directly from
five snapshots (hall, york0, roundtable, map, duel), skipping message boxes and
recording random draws, and `inventory::tests` replays 4290 calls with every
DGROUP write. Gaps: the snapshots share one party (9 carried instances), and
01ff's "no free slot" fall-through and full ground piles are not exercised.

## Next

Leaves left: 078f (needs the `FUN_15b8_0461` row emitter as a host call),
2611 (needs `FUN_1000_0396`, `FUN_332e_000c`, the formatter), 27d0, 0e76, 001c.
Then the menu handlers, which need the menu/list UI as host calls.
