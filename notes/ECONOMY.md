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

`GameState::inventory` mirrors `FUN_1ba8_078f`: carried instances are counted
by definition (and equipped state), so three separate `healing balm` instances
become one row with `quantity == 3`. The item screen prints the count and marks
equipped rows.

## Actions

| action | implementation |
|---|---|
| use | consumables apply definition `+0x17` effect id with `+0x16` magnitude and are removed; equipment is equipped instead |
| drop | carried instance becomes owner `0`, scene = current scene, flags `0x04` |
| pickup | ground instance becomes carried by the selected character |
| give | instance owner is rewritten; gold is moved between purse words |
| equip | item gets flags `0x19`; other equipped items in the same slot revert to `0x18` |
| buy | checks purse, then transfers the merchant's exact stock instance (`FUN_1ba8_0431` owner rewrite); catalogue-only stock needs a free slot and is checked before spending |
| sell | transfers the carried instance to the buyer when supplied, otherwise removes it, then pays half the definition value |
| cast | checks the selected prepared spell and consumes that exact instance slot, then applies its effect |

Shop value is definition `+0x16`, minimum 1. Selling pays half, minimum 1.
Trading never destroys/recreates an item and a failed catalogue purchase leaves
the purse and instance table unchanged.

`FUN_1ba8_27d0` also identifies the stat bytes targeted by effect ids through
the label table at DS `0x4FD8`: `36` magic defense, `37` magical skill, `42`
magical power, `43` strength, `44` life energy, `45` combat ability, `46`
agility, `47` noble bearing and `48` faith. The port maps the playable subset to
`health`, `combat` and `armor` (see `apply_effect`); `4` is Repletion's heal and
`64` the charm/elixir effect.

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
buy/sell, casting, duplicate grouping, exact-instance transfer and failure
cases. Runtime tests drive the status/item/shop/spell panels for every action
and assert the serialized save bytes change. Integration tests exercise give
and Talk on `EXAMPLE.SAV`.
