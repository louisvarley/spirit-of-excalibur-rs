# Gap recovery log

Original scan: **28 gaps >=16 bytes**. Total new functions: **21** (461 -> 482). After prologue/reference recovery and data classification: **4 gaps / 133 bytes** remain.

## 1. Prologue candidates created

| address | result |
|---|---|
| `2036:0009` | created FUN_2036_0009 |
| `2a6e:0009` | created FUN_2a6e_0009 |
| `2d65:0001` | created FUN_2d65_0001 |
| `321b:0004` | created FUN_321b_0004 |
| `321e:000c` | created FUN_321e_000c |
| `332f:000c` | created FUN_332f_000c |
| `3337:0007` | created FUN_3337_0007 |

Large-gap check: `2036:0009` became a single 1132-byte function and `2d65:0001` a single 2211-byte function. No extra `55 8B EC` prologue or `RETF`-separated function was found in either gap, so no further split was needed.

## 2. Reference-based recovery

**239** reference candidates were found inside the original gaps. **15** became new functions (14 recorded below plus `FUN_1ba8_3032`); the rest were already inside a function or duplicated another candidate.

| address | result |
|---|---|
| `1000:0359` | created FUN_1000_0359 |
| `1000:46aa` | created FUN_1000_46aa |
| `1000:46b6` | created FUN_1000_46b6 |
| `1000:9a00` | created FUN_1000_9a00 |
| `1000:9a12` | created FUN_1000_9a12 |
| `1000:c031` | created FUN_1000_c031 |
| `1000:c033` | created caseD_3f |
| `1000:c4c2` | created FUN_1000_c4c2 |
| `1000:df4e` | created FUN_1000_df4e |
| `1000:e044` | created FUN_1000_e044 |
| `1000:eab2` | created FUN_1000_eab2 |
| `2000:b714` | created FUN_2000_b714 |
| `2000:bb27` | created FUN_2000_bb27 |
| `3000:10e9` | created FUN_3000_10e9 |

The 1ba8 orphan switch at `1ba8:324e` belongs to **`FUN_1ba8_3032`** (4378 bytes), found by reference recovery. A provisional `1ba8:3030` function was a 1-byte mid-instruction artifact and was removed.

## 3. Data classification

Only clear data regions were marked, using strings, far-pointer runs, or padding. Code-like gaps were left alone.

| block | start | length | reason |
|---|---|---:|---|
| CODE_15 | `20b5:049e` | 373 | far-pointers |
| CODE_24 | `2d65:2909` | 179 | far-pointers |
| CODE_24 | `2d65:29bf` | 132 | strings |
| CODE_24 | `2d65:2a4a` | 655 | strings |
| CODE_26 | `307a:0049` | 390 | strings |
| CODE_26 | `307a:0262` | 225 | strings |
| CODE_26 | `307a:0499` | 279 | strings |
| CODE_29 | `30f5:00a5` | 244 | strings |

## 4. Remaining gaps >=16 bytes

| block | start | length |
|---|---|---:|
| CODE_0 | `1000:03c3` | 16 |
| CODE_13 | `1ba8:30b2` | 34 |
| CODE_21 | `2b4c:0212` | 44 |
| CODE_21 | `2b4c:025f` | 39 |

