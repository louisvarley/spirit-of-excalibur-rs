# Army battle

Status: **live record layout reversed; round resolution not yet ported.**

## Captured screen

The battle capture shows:

- Round Table units on the left, `Foes...` on the right.
- Foe: `Melehan`, 100 knights, 825 soldiers, 250 archers.
- Bottom message stream, e.g. `SIR BEDIVERE REELS FROM A SHARP BLOW.`
- A miniature battlefield rendering between the text panes.

## Live unit records

Battle units are 0x38-byte records. Verified from the capture:

| offset | field |
|---|---|
| `0x00` | armour/combat field A (40 for knights/soldiers/archers) |
| `0x01` | combat field B |
| `0x02` | combat field C |
| `0x0A..0x15` | NUL-padded name |
| `0x26` | maximum/starting strength |
| `0x28` | current strength |
| `0x2A` | primary battle image id |
| `0x2C` | secondary image id |

Examples from the capture:

```text
knight   strength 100/100  image 0x02A5  icon 0x02BC
soldier  strength 825/825  image 0x02A6  icon 0x02BC
archer   strength 250/250  image 0x02A6  icon 0x02C3
```

The same layout is used for the player-side units (`Constantine`, `Bedivere`,
`Palomides`, etc.).

## Battle UI code

`FUN_20b5_085b` draws the battle screen:

- units from the arrays at `DS:9DC2` / `DS:9DD6`
- Round Table/Foes or Defender/Challenger headings
- `Attack`, `Defend`, `Withdraw`, `Automatic` or `Charge`, `Engage`, `Flank`,
  `Retreat`, `Do Round`, `Ignore`
- message list and miniature battlefield.

Combat resolution is in the `20b5`/`2449` region but is not yet ported.

Implemented model: `soe-engine::battle::BattleUnit`.
