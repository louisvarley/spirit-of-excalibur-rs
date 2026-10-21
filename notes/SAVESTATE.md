# Save-state layout (`EPISODE*.DAT`, `*.SAV`)

Status: **file layout verified; character stats, object definitions, live
instances, money, trade and spell records decoded from the save files and the
`1ba8`/`2b79` routines.** Remaining unknown bits are listed at the end.

Every `EPISODE*.DAT` and `*.SAV` is 9,077 bytes (`0x2375`). Little-endian.

## Verified file offsets

| file offset | size | contents |
|---|---:|---|
| `0x0000` | `0x30` | header / world flags |
| `0x0030` | 92 x `0x38` | character/NPC records (ends `0x1450`) |
| `0x1A4F` | 80 x `0x06` | event/dialogue triggers (ends `0x1C2F`) |
| `0x1C4A` | 49 x `0x1B` | object definitions (items and spells, ends `0x2175`) |
| `0x2175` | 85 x `0x04` | live object instances (ends `0x22C9`) |
| `0x22C9` | 50 x `0x03` | relationships `(A, B, value)` (ends `0x235F`) |
| `0x235F` | `0x16` | 22-byte tail |

Names in `EPISODE1.DAT` confirm the tables: characters 0/1 are
`Constantine`/`Bedivere`, location 0 is `Colchester`, object 1 is
`gold besant`, and object 36 is `Repletion`.

## DGROUP mapping (confirmed)

The loader is `FUN_1a51_059d` (image offset `0xAAAD`). It copies:

| file range | bytes | DS destination |
|---|---:|---|
| `0x0000..0x235E` | 9055 | `0x2C5C..0x4FBA` |
| `0x235F..0x2374` | 22 | `0x4FBF..0x4FD4` |

So file `0x1C4A` maps to DS `0x48A6`, and the original's 1-based object
pointer `0x488B + index*0x1B` points at object 1 for `index == 1`. The
character pointer `0x2C5E + index*0x38` points at the **name** field of
character `index` (character 1 -> DS `0x2C96`), which is why the code reads
`+0x28` for gold while the file record stores gold at `+0x32`.

## Character record `0x38`

| record offset | contents |
|---|---|
| `+0x00` | health / current HP |
| `+0x01` | hits (max/health pool; status header `hth`) |
| `+0x02` | movement (`MV`) |
| `+0x03` | weapon/readied item index (status sword column) |
| `+0x04` | combat (`COM`) |
| `+0x05` | armour (`ARM`) |
| `+0x06` | nobility (`NOB`) |
| `+0x07` | faith (`FTH`) |
| `+0x08` | magic skill |
| `+0x09` | level/rank |
| `+0x0A` | 12-byte NUL-padded name |
| `+0x20` | runtime presence flags (`+0x16` in runtime coordinates) |
| `+0x32` | party purse, little-endian word (`+0x28` in runtime coordinates) |
| rest | sprite, map coordinates, status and runtime fields |

The status screenshot columns are `hp hth MV [weapon] COM ARM NOB FTH`.
Equipment is **not** stored in the character record: it is the set of carried
object instances with the equipped bit set (below). Effective combat adds the
equipped weapon's power; effective armour adds the equipped armour effects.

## Object definition `0x1B`

There is one contiguous 49-record table. Items and prepared spells share it;
a spell is simply an object with `kind == 0` and `subtype == 0`. Later saves
(`D3.SAV`, `TRY1.SAV`) use more than the first six spell-looking slots, so the
old "35 items + 6 spells" split was wrong.

| definition offset | contents |
|---|---|
| `+0x00` | 18-byte NUL-padded name |
| `+0x12` | kind (see below) |
| `+0x13` | subtype: `3` item, `0` spell |
| `+0x14` | power / weapon damage / potion strength |
| `+0x15` | item class/effect group |
| `+0x16` | effect magnitude, also used as shop value |
| `+0x17` | effect handler id (`44` heal, `43` vitality, `45` combat, `46` armour, `64` charm) |
| `+0x18` | 16-bit definition flags (bit `0x20` enchanted, etc.) |
| `+0x1A` | spell component bitmask |

Verified kinds: `49` money, `51` key, `52` map, `55..=57` swords/knives,
`58` shield, `59` helm, `60..=61` body armour, `62` gauntlets, `33/34`
potions, `35` ingredients, `39..=47` miscellaneous/quest objects, `0` spell.

Definition 1 is always `gold besant`; its instance byte `+0x02` is the amount
when the original creates a money pickup, while the authoritative party purse
is character `+0x32`.

## Live object instance `0x04`

| instance offset | contents |
|---|---|
| `+0x00` | object definition index, 1-based (`0` = empty slot) |
| `+0x01` | owner character index, 1-based (`0` = ground/scenery) |
| `+0x02` | scene id for normal objects; amount for the money record |
| `+0x03` | flags |

Verified flag values:

| flags | meaning |
|---:|---|
| `0x18` | carried in the owner's inventory |
| `0x19` | carried and equipped |
| `0x04` | lying in a scene / container |
| `0x01`, `0x02` | fixed scenery or shop/quest placement |
| `0x0a`, `0x0b` | merchant/stock placement |
| `0x0d` | money placement |

`FUN_1ba8_04fd` counts carried instances (`flags == 0x18`), `FUN_1ba8_078f`
lists a character's inventory, and `FUN_1ba8_0431` transfers an item by
rewriting `+0x01` (or moves the gold word between characters when the object
is definition 1). `FUN_1ba8_01ff` finds/creates the money instance and
`FUN_1ba8_1005` implements trade using these lists and definition `+0x16` as
the price. `FUN_1ba8_44f1` -> `FUN_2b79_0a7f` selects a prepared spell;
`FUN_1ba8_2e27` consumes the selected spell and applies its effect.

## Events / relationships

`0x1A4F` holds 80 six-byte triggers `{actor, relation, flags, line}`. `actor`
and `line` are 1-based character and `HINT.RES` record indices; bits `0x70`
are priority, `0x2000` a plea and `0x4000` a join offer. `0x22C9` keeps the
50 three-byte relationships.

## Unknown / still approximate

- Some definition flag bits (`0x20`, `0x40`, `0x80`, high word bits) are not
  individually named yet; only equipment, magical and component behaviour is
  used by the port.
- Instance flags `0x01`/`0x02`/`0x0a`/`0x0b` are observed placements but their
  exact original semantics (fixed scenery vs. merchant stock) are inferred.
- The stat names `hits`, `nobility` and `level` follow the status table and
  code use; the original manual wording may differ.
