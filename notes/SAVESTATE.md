# Save-state layout (`EPISODE*.DAT`, `*.SAV`)

Status: **file layout verified; DGROUP mapping partly unverified**.

Every `EPISODE*.DAT` and `*.SAV` is 9,077 bytes (`0x2375`). Little-endian.

## Verified file offsets

| file offset | size | contents |
|---|---:|---|
| `0x0000` | `0x30` | header / world flags |
| `0x0030` | 64 x `0x38` | character records (ends `0x0E30`) |
| `0x1473` | 49 x `0x19` | location records (ends `0x195C`) |
| `0x1C4A` | 35 x `0x1B` | item records (ends `0x1FFB`) |
| `0x1FFB` | 6 x `0x1B` | spell records (ends `0x209D`) |
| `0x2175` | — | event/flag table (per prior notes; unverified) |
| `0x22C9` | 50 x 3 | relationships `(A, B, value)` (per prior notes) |

Verified by reading `EPISODE1.DAT`:

- character 0 at `0x30`: name at `+0x0A` = `Constantine`
- character 1 at `0x68`: `Bedivere`
- location 0 at `0x1473`: `Colchester`, then `Gloucester`, `Lincoln`, `York`, `London`
- item 0 at `0x1C4A`: `gold besant`, then `knife`, `broadsword`, `runic blade`, `Excalibur`
- spell 0 at `0x1FFB`: `Repletion`, `Charm`, `Summon Undine`, `Heal`, `Defend`, `Shield`

## DGROUP mapping (prior claim — not fully verified)

Prior notes say the loader copies:

| file range | DGROUP |
|---|---|
| `0x0000..0x0015` (22 bytes) | `0x4FBF` |
| `0x0016..end` (9,055 bytes) | `0x2C5C..0x4FBA` |

Arithmetic check: `0x2C5C + 0x235F = 0x4FBB`, one byte past the stated
`0x4FBA`, and the 22-byte block overlaps that range by 0x16. The prior notes
themselves flag a 0x16 contradiction. The loader image offset is claimed at
`0xAAAD`; we have not yet matched every byte against DGROUP, so the mapping is
**unverified**.

Consequences if the claim is right:

- character 0 file `0x30` -> DGROUP `0x2C8C`; names at `0x2C96`; the code's
  `0x2C5E` base is one record earlier (1-based indexing).
- location 0 file `0x1473` -> DGROUP `0x40CF` (the runtime table starts one
  0x19-byte header entry earlier at `0x40B6`).
- item 0 file `0x1C4A` -> DGROUP `0x48A6`.
- flag/event table file `0x2175` -> DGROUP `0x4DD1`.
- relationships file `0x22C9` -> DGROUP `0x4F25`.

## Character record `0x38`

Stats at `+0x00..+0x09`, 12-byte NUL-padded name at `+0x0A`, home/start
positions at `+0x16`/`+0x1A`, sprite descriptor at `+0x1E`, status at `+0x22`,
save/world X/Y at `+0x28`/`+0x2A`/`+0x2C`, remaining runtime state at `+0x2E`.
Character indices are 1-based in the original.

## Location `0x19`

Name 18 bytes, world X/Y at `+0x12`/`+0x14` (four times map pixels), type at
`+0x16`, flags at `+0x18`.

## Item/spell `0x1B`

Name 18 bytes, kind at `+0x12`, subtype `+0x13`, power `+0x14`, effect fields
`+0x15..+0x1A`.

## Flags / relationships

The flag table at `0x2175` and 50 x 3 relationship records at `0x22C9` come from
the prior notes. They are **unverified** until the loader mapping is confirmed
against DGROUP via the oracle.


## Follow-up 10 cross-check

File layout above is **confirmed** by names in `EPISODE1.DAT` (characters
`Constantine`/`Bedivere`, locations `Colchester`/`Gloucester`, items `gold
besant`/`knife`, spells `Repletion`/`Charm`). Character name field is at
record `+0x0A` (12 bytes).

### Corrected DGROUP mapping (CONFIRMED by loader arithmetic)

The loader is `FUN_1a51_059d` (image offset `0xAAAD`). It copies:

| file range | bytes | DS destination |
|---|---:|---|
| `0x0000..0x235E` | 9055 | `0x2C5C..0x4FBA` |
| `0x235F..0x2374` | 22 | `0x4FBF..0x4FD4` |

The 22-byte block is the **file tail**, not the head. The prior 0x16
contradiction came from treating it as a file prefix. This makes character 0
(file `0x30`) land at DS `0x2C8C`, name at `0x2C96`, and matches the code's
1-based `0x2C5E + index*0x38` naming (`index 1 -> 0x2C96`).

`soe-engine::save::GameState` implements the loader and `dgroup_image()`; its
test checks `Constantine` at DS `0x2C96` and the 22-byte tail at `0x4FBF`.
