# Function pointers and zero-caller coverage

Sources: initialized DGROUP 4-byte far-pointer scan, plus the MZ relocation table (`notes/reloc_pointers_raw.tsv`).

## Summary

- Relocations in table: **2598**; segment-valued words pointing at real code segments: **2381**
- Unique function-pointer pairs found: **2364**; unique target functions: **319**
- Data pointers: **79** (2 tables, 60 isolated)
- Code pointers: **2228** CALLF/immediate + **57** MOV/PUSH pairs
- Zero-caller functions now: **107**
- Explained by a pointer: **40**
- Still unexplained: **67**

DGROUP-only scan explained 15; the relocation scan raises this to 40, so it adds 25 more zero-caller functions.

## Data pointer tables

### `3364:0389` - 16 entries, 15 unique

| offset word | target | function |
|---|---|---|
| `3364:0389` | `18cb:08ff` | FUN_18cb_08ff |
| `3364:038d` | `18cb:0944` | FUN_18cb_0944 |
| `3364:0391` | `18cb:0b06` | FUN_18cb_0b06 |
| `3364:0395` | `18cb:0bb1` | FUN_18cb_0bb1 |
| `3364:0399` | `18cb:0bdb` | FUN_18cb_0bdb |
| `3364:039d` | `18cb:0c2d` | FUN_18cb_0c2d |
| `3364:03a1` | `18cb:0c8a` | FUN_18cb_0c8a |
| `3364:03a5` | `18cb:0de6` | FUN_18cb_0de6 |
| `3364:03a9` | `18cb:0e5f` | FUN_18cb_0e5f |
| `3364:03ad` | `18cb:0e5f` | FUN_18cb_0e5f |
| `3364:03b1` | `18cb:15c3` | FUN_18cb_15c3 |
| `3364:03b5` | `18cb:15d3` | FUN_18cb_15d3 |
| `3364:03b9` | `18cb:1201` | FUN_18cb_1201 |
| `3364:03bd` | `18cb:13c4` | FUN_18cb_13c4 |
| `3364:03c1` | `18cb:15e3` | FUN_18cb_15e3 |
| `3364:03c5` | `18cb:15ea` | FUN_18cb_15ea |

### `3364:79a6` - 3 entries, 2 unique

| offset word | target | function |
|---|---|---|
| `3364:79a6` | `1772:0578` | FUN_1772_0578 |
| `3364:79aa` | `1772:0578` | FUN_1772_0578 |
| `3364:79ae` | `2920:1067` | FUN_2920_1067 |

## Isolated data pointers

| offset word | target | function |
|---|---|---|
| `20b5:04f1` | `2449:0410` | FUN_2449_0410 |
| `20b5:0540` | `1772:0623` | FUN_1772_0623 |
| `20b5:054f` | `1ba8:44f1` | FUN_1ba8_44f1 |
| `20b5:055b` | `1ba8:43dc` | FUN_1ba8_43dc |
| `20b5:0566` | `15b8:09f7` | FUN_15b8_09f7 |
| `20b5:056d` | `1772:072b` | FUN_1772_072b |
| `20b5:0608` | `14b0:03f5` | FUN_14b0_03f5 |
| `2a6e:0016` | `2d65:08a4` | FUN_2d65_08a4 |
| `2a6e:0038` | `1772:000e` | FUN_1772_000e |
| `2b4c:027d` | `1772:000e` | FUN_1772_000e |
| `2d65:0014` | `1316:187b` | FUN_1316_187b |
| `2d65:0063` | `2d65:13e2` | FUN_2d65_13e2 |
| `2d65:0068` | `2d65:2048` | FUN_2d65_2048 |
| `2d65:00cc` | `2d65:13e2` | FUN_2d65_13e2 |
| `2d65:00d4` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:00da` | `2d65:1ff0` | FUN_2d65_1ff0 |
| `2d65:00e2` | `15b8:179c` | FUN_15b8_179c |
| `2d65:00eb` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:00f1` | `1772:0578` | FUN_1772_0578 |
| `2d65:00f6` | `1316:1143` | FUN_1316_1143 |
| `2d65:00ff` | `14b0:0122` | FUN_14b0_0122 |
| `2d65:012c` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:013a` | `14b0:02c4` | FUN_14b0_02c4 |
| `2d65:015c` | `14b0:0122` | FUN_14b0_0122 |
| `2d65:0197` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:01a5` | `14b0:02c4` | FUN_14b0_02c4 |
| `2d65:01bf` | `1316:1143` | FUN_1316_1143 |
| `2d65:01c4` | `17f3:003a` | FUN_17f3_003a |
| `2d65:01ee` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:01f6` | `1316:1111` | FUN_1316_1111 |
| `2d65:01fe` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:0219` | `15b8:179c` | FUN_15b8_179c |
| `2d65:0224` | `15b8:179c` | FUN_15b8_179c |
| `2d65:022f` | `15b8:179c` | FUN_15b8_179c |
| `2d65:0235` | `17f3:003a` | FUN_17f3_003a |
| `2d65:2950` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:298b` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:29bf` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2a0f` | `3360:0000` | FUN_3360_0000 |
| `2d65:2a1c` | `3360:0000` | FUN_3360_0000 |
| `2d65:2a50` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:2aa3` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2ad6` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2b0d` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2b2c` | `3360:0000` | FUN_3360_0000 |
| `2d65:2b39` | `3360:0000` | FUN_3360_0000 |
| `2d65:2b99` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2bd2` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2c09` | `14b0:0bbf` | FUN_14b0_0bbf |
| `2d65:2c11` | `17f3:0097` | FUN_17f3_0097 |
| `2d65:2c2b` | `15b8:09f7` | FUN_15b8_09f7 |
| `2d65:2c53` | `15b8:09f7` | FUN_15b8_09f7 |
| `2d65:2c5f` | `17f3:003a` | FUN_17f3_003a |
| `2d65:2c67` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:2c79` | `15b8:1876` | FUN_15b8_1876 |
| `2d65:2c87` | `105e:08ae` | FUN_105e_08ae |
| `2d65:2c94` | `15b8:0978` | FUN_15b8_0978 |
| `321b:002a` | `3208:000d` | FUN_3208_000d |
| `321e:0032` | `32b2:000a` | FUN_32b2_000a |
| `332f:0020` | `3119:0000` | FUN_3119_0000 |

## Code pointers

- `2228` direct CALLF/immediate references and `57` MOV/PUSH immediate pairs.
- These are individual references, not contiguous tables; the full list is in `notes/reloc_pointers_raw.tsv`.
- Many are ordinary direct calls; the zero-caller benefit comes from references located in orphan/unanalysed code or in data stores.

## Zero-caller functions explained

| function | address | size | first pointer |
|---|---|---:|---|
| entry | `1000:0000` | 269 | `1000:01c7` (code) |
| FUN_1316_13d2 | `1316:13d2` | 78 | `1316:034f` (code-pair) |
| FUN_15b8_00bb | `15b8:00bb` | 152 | `15b8:0089` (code-pair) |
| FUN_15b8_0153 | `15b8:0153` | 410 | `15b8:0060` (code-pair) |
| FUN_15b8_02ed | `15b8:02ed` | 372 | `15b8:0058` (code-pair) |
| FUN_15b8_18ef | `15b8:18ef` | 157 | `2d65:2db7` (code-pair) |
| FUN_15b8_198c | `15b8:198c` | 105 | `2d65:2ddd` (code-pair) |
| FUN_18cb_08ff | `18cb:08ff` | 69 | `3364:0389` (data) |
| FUN_18cb_0944 | `18cb:0944` | 450 | `3364:038d` (data) |
| FUN_18cb_0b06 | `18cb:0b06` | 171 | `3364:0391` (data) |
| FUN_18cb_0bb1 | `18cb:0bb1` | 42 | `3364:0395` (data) |
| FUN_18cb_0bdb | `18cb:0bdb` | 82 | `3364:0399` (data) |
| FUN_18cb_0c2d | `18cb:0c2d` | 93 | `3364:039d` (data) |
| FUN_18cb_0de6 | `18cb:0de6` | 7 | `3364:03a5` (data) |
| FUN_18cb_0e5f | `18cb:0e5f` | 866 | `3364:03a9` (data) |
| FUN_18cb_1201 | `18cb:1201` | 431 | `3364:03b9` (data) |
| FUN_18cb_13c4 | `18cb:13c4` | 495 | `3364:03bd` (data) |
| FUN_18cb_15c3 | `18cb:15c3` | 16 | `3364:03b1` (data) |
| FUN_18cb_15d3 | `18cb:15d3` | 6 | `3364:03b5` (data) |
| FUN_18cb_15e3 | `18cb:15e3` | 7 | `3364:03c1` (data) |
| FUN_18cb_15ea | `18cb:15ea` | 7 | `3364:03c5` (data) |
| FUN_1a51_096a | `1a51:096a` | 198 | `2d65:10fd` (code-pair) |
| FUN_1ba8_43dc | `1ba8:43dc` | 277 | `20b5:055b` (data) |
| FUN_1ba8_44f1 | `1ba8:44f1` | 272 | `20b5:054f` (data) |
| FUN_1ba8_4604 | `1ba8:4604` | 741 | `2864:0bac` (code-pair) |
| FUN_2449_0420 | `2449:0420` | 210 | `20b5:0c38` (code-pair) |
| FUN_2449_04f2 | `2449:04f2` | 121 | `20b5:0c6b` (code-pair) |
| FUN_2449_056b | `2449:056b` | 207 | `20b5:0ca2` (code-pair) |
| FUN_2449_063a | `2449:063a` | 204 | `20b5:0cda` (code-pair) |
| FUN_2449_0764 | `2449:0764` | 22 | `20b5:0d12` (code-pair) |
| FUN_2449_0792 | `2449:0792` | 682 | `20b5:0a81` (code-pair) |
| FUN_2449_0ade | `2449:0ade` | 15 | `20b5:0c0a` (code-pair) |
| FUN_2864_006e | `2864:006e` | 932 | `2d65:10c6` (code-pair) |
| FUN_2864_0a7e | `2864:0a7e` | 277 | `2d65:112f` (code-pair) |
| FUN_2864_0b93 | `2864:0b93` | 49 | `2d65:213b` (code-pair) |
| FUN_2920_026c | `2920:026c` | 1063 | `2d65:2d07` (code-pair) |
| FUN_2920_0738 | `2920:0738` | 19 | `26c5:0bf3` (code-pair) |
| FUN_2920_0a12 | `2920:0a12` | 1621 | `2d65:2cf7` (code-pair) |
| FUN_2d65_0bc8 | `2d65:0bc8` | 345 | `2920:1159` (code-pair) |
| FUN_2d65_2048 | `2d65:2048` | 585 | `2d65:0068` (data) |

## Zero-caller functions still unexplained

| function | address | size | segment |
|---|---|---:|---|
| FUN_105e_0f3b | `105e:0f3b` | 86 | 105e |
| FUN_105e_0f91 | `105e:0f91` | 84 | 105e |
| FUN_1316_03bd | `1316:03bd` | 5 | 1316 |
| FUN_1316_0d87 | `1316:0d87` | 758 | 1316 |
| FUN_1316_18d7 | `1316:18d7` | 77 | 1316 |
| FUN_1316_1924 | `1316:1924` | 80 | 1316 |
| FUN_15b8_182f | `15b8:182f` | 71 | 15b8 |
| FUN_1a51_0229 | `1a51:0229` | 884 | 1a51 |
| FUN_1a51_06c5 | `1a51:06c5` | 217 | 1a51 |
| FUN_1a51_079e | `1a51:079e` | 460 | 1a51 |
| FUN_1ba8_0058 | `1ba8:0058` | 16 | 1ba8 |
| FUN_1ba8_00ad | `1ba8:00ad` | 338 | 1ba8 |
| FUN_1ba8_0a9a | `1ba8:0a9a` | 193 | 1ba8 |
| FUN_1ba8_0b5b | `1ba8:0b5b` | 547 | 1ba8 |
| FUN_1ba8_0d7e | `1ba8:0d7e` | 201 | 1ba8 |
| FUN_1ba8_0e47 | `1ba8:0e47` | 47 | 1ba8 |
| FUN_1ba8_1005 | `1ba8:1005` | 1771 | 1ba8 |
| FUN_1ba8_16f2 | `1ba8:16f2` | 104 | 1ba8 |
| FUN_1ba8_19a5 | `1ba8:19a5` | 400 | 1ba8 |
| FUN_1ba8_1b35 | `1ba8:1b35` | 46 | 1ba8 |
| FUN_1ba8_1b63 | `1ba8:1b63` | 300 | 1ba8 |
| FUN_1ba8_1df0 | `1ba8:1df0` | 203 | 1ba8 |
| FUN_1ba8_214b | `1ba8:214b` | 201 | 1ba8 |
| FUN_1ba8_2214 | `1ba8:2214` | 47 | 1ba8 |
| FUN_1ba8_2243 | `1ba8:2243` | 172 | 1ba8 |
| FUN_1ba8_22ef | `1ba8:22ef` | 102 | 1ba8 |
| FUN_1ba8_2355 | `1ba8:2355` | 381 | 1ba8 |
| FUN_1ba8_2ad2 | `1ba8:2ad2` | 213 | 1ba8 |
| FUN_1ba8_2c0f | `1ba8:2c0f` | 395 | 1ba8 |
| FUN_1ba8_2d9a | `1ba8:2d9a` | 102 | 1ba8 |
| FUN_1ba8_2e00 | `1ba8:2e00` | 39 | 1ba8 |
| caseD_0 | `1ba8:3a0b` | 27 | 1ba8 |
| FUN_20b5_0068 | `20b5:0068` | 196 | 20b5 |
| FUN_20b5_0136 | `20b5:0136` | 10 | 20b5 |
| FUN_20b5_02ae | `20b5:02ae` | 279 | 20b5 |
| FUN_20b5_03c5 | `20b5:03c5` | 61 | 20b5 |
| FUN_20b5_0402 | `20b5:0402` | 123 | 20b5 |
| FUN_20b5_047d | `20b5:047d` | 9 | 20b5 |
| FUN_20b5_0613 | `20b5:0613` | 584 | 20b5 |
| FUN_20b5_1622 | `20b5:1622` | 227 | 20b5 |
| FUN_20b5_2bd1 | `20b5:2bd1` | 53 | 20b5 |
| FUN_2449_00f7 | `2449:00f7` | 16 | 2449 |
| FUN_2449_0706 | `2449:0706` | 47 | 2449 |
| FUN_2449_0735 | `2449:0735` | 47 | 2449 |
| FUN_2449_0a3c | `2449:0a3c` | 52 | 2449 |
| FUN_2449_12de | `2449:12de` | 85 | 2449 |
| FUN_26c5_111d | `26c5:111d` | 2266 | 26c5 |
| FUN_2864_0412 | `2864:0412` | 1335 | 2864 |
| FUN_2864_0a6e | `2864:0a6e` | 16 | 2864 |
| FUN_2d65_0aad | `2d65:0aad` | 27 | 2d65 |
| FUN_2d65_0d34 | `2d65:0d34` | 5 | 2d65 |
| FUN_2d65_108e | `2d65:108e` | 27 | 2d65 |
| FUN_2d65_11ad | `2d65:11ad` | 59 | 2d65 |
| FUN_2d65_11e8 | `2d65:11e8` | 175 | 2d65 |
| FUN_2d65_1297 | `2d65:1297` | 214 | 2d65 |
| FUN_2d65_1395 | `2d65:1395` | 60 | 2d65 |
| FUN_2d65_13d1 | `2d65:13d1` | 17 | 2d65 |
| FUN_2d65_2291 | `2d65:2291` | 635 | 2d65 |
| FUN_2d65_250c | `2d65:250c` | 1021 | 2d65 |
| FUN_30d5_0146 | `30d5:0146` | 48 | 30d5 |
| FUN_311c_003f | `311c:003f` | 47 | 311c |
| FUN_316b_024e | `316b:024e` | 59 | 316b |
| FUN_316b_0289 | `316b:0289` | 66 | 316b |
| FUN_31b6_00e4 | `31b6:00e4` | 113 | 31b6 |
| FUN_3283_0074 | `3283:0074` | 73 | 3283 |
| FUN_32c1_01c3 | `32c1:01c3` | 25 | 32c1 |
| FUN_3337_0028 | `3337:0028` | 36 | 3337 |

