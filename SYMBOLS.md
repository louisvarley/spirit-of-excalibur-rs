# Symbols

Address -> name, signature, purpose. Keep this sorted by address. Add a line when you learn something; mark confidence as `?` (guess), `~` (likely), `=` (verified by trace test).

Signature notation: `ret name(args)`, args in source order, `far` where it returns a far pointer.

## Functions

| Address | Name | Signature | Purpose | Conf |
|---|---|---|---|---|
| 1000:0000 | entry | `void entry()` | Turbo C startup, ends by calling main | ~ |
| 1000:03f1 | (hub, 39 callers) | ? | tiny helper, called from many places | ? |
| 1000:0396 | (hub, 29 callers) | ? | tiny helper | ? |
| 1772:0578 | (hub, 66 callers) | `?` | called first thing in many functions, probably a runtime/UI service | ? |
| 1772:000e | (hub, 52 callers) | ? | takes a message string, coordinates and flags | ? |
| 14b0:0bbf | (hub, 43 callers) | ? | 1221 bytes | ? |
| 15b8:0461 | (hub, 36 callers) | `far ptr f(...)` | returns a far pointer to a record, takes many args | ? |
| 15b8:16ca | (hub, 30 callers) | `far ptr f(a, b, c)` | called with 3 stack args from 18cb:0944 | ? |
| 2d65:08a4 | (hub, 31 callers) | ? | called before building messages | ? |
| 18cb:0944 | (unnamed) | `u16 f(u16 flags)` | moves the current entity by flags (0x1ff magnitude, 0x400 sign, 0x800 axis); returns 0 on arrival, 2 otherwise | ~ |

## Globals (DGROUP, segment 3364)

| Address | Type | Purpose | Conf |
|---|---|---|---|
| 3364:9a6e | far ptr | current record/entity | ~ |
| 3364:9a1c | u16 | selector used by the 2d65:0ac8 switch (1-4) | ? |
| 3364:2c5e.. | records, stride 0x38 | character/entity table | ~ |
| 3364:40cf.. | records, stride 0x19 | locations | ~ |
| 3364:48a6.. | records, stride 0x1b | items | ~ |
| 3364:0389 | far ptr table, 16 entries | handlers in segment 18cb | ~ |
| 3364:79a6 | far ptr table, 3 entries | 1772:0578, 2920:1067 | ? |

## Structs

(none documented yet — add as you learn them, with offsets and sizes)
