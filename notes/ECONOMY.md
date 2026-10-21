# Inventory, equipment, money, trade and magic

Status: **playable economy loop implemented** in `soe-engine::economy` and
wired into the status/item/spell/trade screens.

## Data model

`save::GameState` now parses the full save layout described in
`notes/SAVESTATE.md`:

- `items`: 49 object definitions (items and spells share this table).
- `instances`: 85 live objects `{definition, owner, scene_or_amount, flags}`.
- `events`: 80 dialogue/event triggers.
- `Character`: decoded stat bytes and the purse word at file `+0x32`.

`ObjectInstance::is_carried()` tests `flags & 0x18 == 0x18`;
`is_equipped()` additionally requires bit `0x01`.

## Actions

| action | implementation |
|---|---|
| use | consumables apply definition `+0x17` effect id with `+0x16` magnitude and are removed; equipment is equipped instead |
| drop | carried instance becomes owner `0`, scene = current scene, flags `0x04` |
| pickup | ground instance becomes carried by the selected character |
| give | instance owner is rewritten; gold is moved between purse words |
| equip | item gets flags `0x19`; other equipped items in the same slot revert to `0x18` |
| buy | checks purse, removes merchant stock when a seller is supplied, creates a carried instance |
| sell | removes a carried instance, pays half the definition value, optionally gives it to the buyer |
| cast | checks for the selected prepared spell, consumes that instance, then applies its heal/defend/charm effect |

Shop value is definition `+0x16`, minimum 1. Selling pays half, minimum 1.
`GameState::character_sheet` exposes base and effective combat/armour plus
equipment, inventory and prepared spells for the live status screen.

## UI

- `I` opens the Round Table status. Up/down selects a knight; the panel shows
  decoded stats, effective combat/armour, purse, gear and prepared spells.
  Enter opens that knight's inventory.
- Room action menu: Pickup/Drop/Use open the item list, Magic opens the spell
  list, Trade selects a target then opens the buy/sell panel, Give selects a
  target then opens the item list.
- `Space` toggles buy/sell in the trade panel. `Enter` performs the highlighted
  action. Cancel returns to the room.

## Tests

`crates/soe-engine/src/economy.rs` covers use, drop/pickup, give, equip,
buy/sell, casting and failure cases. Runtime tests cover the action/screen
routing. Integration tests exercise give and Talk on `EXAMPLE.SAV`.
