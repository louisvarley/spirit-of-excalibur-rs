# Spirit of Excalibur — Ghidra analysis notes

Date: 2026-10-20

## Backup

- `ghidra_backup_20261020.zip` created from `ghidra/SOE.rep` + `ghidra/SOE.gpr` before any project change.

## 1. Verification: is `3364:0000-81bb` (CODE_86) really DGROUP?

Evidence from the existing export (before any change):

- Block `CODE_86`: `3364:0000`–`3364:81bb`, 33212 bytes, marked `rwx`, initialized.
- Followed immediately by block `DATA`: `3364:81bc`–`3364:b96b`, 14256 bytes, `rw-`, **uninitialized** (classic DGROUP: initialized data then BSS).
- `strings.tsv` has **498 strings** in segment 3364, e.g.:
  - `3364:0004` "Turbo-C - Copyright (c) 1988 Borland Intl."
  - `3364:002f` "Divide error\r\nAbnormal program termination\r\n"
  - `3364:010d` "Resource not found: "
  - `3364:0198` "game.cfg"
  - `3364:0210` "Do you really wish to quit the game?"
- Only 4 functions were created in 3364, all 1 byte and **0 instructions**:
  `3364:a011`, `3364:a9e4`, `3364:b2a7`, `3364:b3f2`.
- Startup code at `1000:0000` executes `MOV DX,0x3364` right at entry, i.e. it establishes DS = 0x3364.
- Code throughout references `3364:xxxx` as data (symbols `DAT_3364_*`, `s_*_3364_*`).

Exact Ghidra counts for `CODE_86` (read-only `Verify3364.java`):

- instructions: **0**
- defined data items: **907**
- string items: **498**
- functions: **0**
- block flags: read/write/**execute**, initialized

Important correction: the four bogus functions `3364:a011/a9e4/b2a7/b3f2` are **not** in CODE_86. They lie in the adjacent uninitialized `DATA` block (`3364:81bc`–`3364:b96b`, BSS), so they are not fixed merely by changing CODE_86. They are relocation/image-base aliases of real code in segment 2d65 (`2d65:0001`, `09d4`, `1297`, `13e2`).

Conclusion: **CONFIRMED** — CODE_86 is the initialized DGROUP data segment, not code. It should be rw-, non-execute. The four pseudo-functions in BSS should be removed separately.

### Fix applied

- Restored `SOE.rep` from the backup once after an initial attempt used `clearCodeUnits`, which wiped all 907 defined data items (Ghidra also clears data at those addresses). Reapplied without clearing, since `instructions=0` (nothing to clear).
- Renamed `CODE_86` → `DGROUP_DATA`, set `rw-`, execute off.
- Removed the four 1-byte BSS pseudo-functions `3364:a011/a9e4/b2a7/b3f2`.
- Verified afterwards: `DGROUP_DATA` = 0 instructions, **907 defined data items**, **498 strings**, 0 functions.
- Re-export result:
  - functions: **465 → 461** (only the four bogus BSS entries removed)
  - `.md` files: 461 (no stale files)
  - decompile failures: **still 4** — `1237:0d62`, `1ba8:2e27`, `2d65:0bc8`, `2d65:13e2`
  - `strings.tsv`: 498 strings preserved, all in segment 3364
  - `segments.tsv`: `DGROUP_DATA 3364:0000-3364:81bb 33212 rw- true`; `DATA 3364:81bc-3364:b96b 14256 rw- false`


## 3. Platform and I/O entry points

### INT 21h

| AH | meaning | calls | function entry (call addresses) |
|---|---|---:|---|
| `0x9` | print $-string | 3 | 307a:0353 (307a:036a, 307a:037b, 307a:03c9) |
| `0xe` | select disk | 1 | 332f:0031 (332f:0039) |
| `0x1a` | set DTA | 2 | 3333:0001 (3333:000a),3333:0027 (3333:0030) |
| `0x1c` | get drive data | 1 | 3337:0028 (3337:0031) |
| `0x25` | set interrupt vector | 11 | 1000:012f (1000:016e),1000:0172 (1000:017a, 1000:0185, 1000:0190, 1000:019b),307a:0000 (307a:0042),307a:01cf (307a:01e5),30f5:0010 (30f5:0027),30f5:004a (30f5:0075),3360:0000 (3360:0017, 3360:0031) |
| `0x30` | DOS version | 1 | 1000:0000 (1000:000a) |
| `0x35` | get interrupt vector | 7 | 1000:012f (1000:0133, 1000:0140, 1000:014d, 1000:015a),307a:0000 (307a:0029),30f5:004a (30f5:0056),3360:0000 (3360:000c) |
| `0x36` | get free space | 1 | 3337:004c (3337:0055) |
| `0x3c` | create file | 1 | 3197:000b (3197:0017) |
| `0x3d` | open file | 1 | 31b1:000b (31b1:0031) |
| `0x3e` | close file | 1 | 3218:000e (3218:0019) |
| `0x3f` | read file | 1 | 3247:0009 (3247:0018) |
| `0x40` | write file | 3 | 1000:01a7 (1000:01ac),3197:002a (3197:0036),3268:0009 (3268:0037) |
| `0x41` | delete file | 1 | 328e:000d (328e:0016) |
| `0x42` | lseek | 7 | 3271:0004 (3271:0020),32ab:000a (32ab:0039, 32ab:0046, 32ab:0055),3326:0009 (3326:0019, 3326:0026, 3326:0035) |
| `0x43` | get/set file attributes | 1 | 32a9:0009 (32a9:0018) |
| `0x44` | IOCTL | 3 | 31f0:0006 (31f0:0018),31f3:0003 (31f3:000c),32ab:000a (32ab:0029) |
| `0x4a` | resize memory | 2 | 1000:0000 (1000:00b2),3169:0000 (3169:000b) |
| `0x4c` | exit | 1 | 1000:010d (1000:0123) |
| `0x4e` | find first | 1 | 3333:0001 (3333:0014) |
| `0x4f` | find next | 1 | 3333:0027 (3333:0034) |

### INT 10h

| AX/AH | function entry | call address |
|---|---|---|
| `0x3` | 1316:0002 | 1316:00b4 |
| `0x3` | 1316:0365 | 1316:03a7 |
| `0x1a00` | 306a:0037 | 306a:003a |
| `0x1200` | 306a:0037 | 306a:0050 |
| `0x4` | 307a:0353 | 307a:0359 |
| `AH=0x2` | 307a:0353 | 307a:0363 |
| `AH=0x2` | 307a:0353 | 307a:0374 |
| `AH=0x2` | 307a:0353 | 307a:03c2 |

### INT 33h

| AX | function entry | call address |
|---|---|---|
| `0x0` | 307a:01ec | 307a:0243 |

### IN / OUT ports

| port | function entry | instructions (address: mnemonic) |
|---|---|---|
| `0x201` | 307a:0428 | 307a:042e OUT DX,AL; 307a:043d IN AL,DX; 307a:0456 IN AL,DX; 307a:046f IN AL,DX; 307a:0489 IN AL,DX |
| `0x3da` | 1316:1302 | 1316:1365 IN AL,DX; 1316:136d IN AL,DX; 1316:13a6 IN AL,DX; 1316:13ae IN AL,DX |
| `0x40` | 30f5:0010 | 30f5:003f OUT 0x40,AL; 30f5:0041 OUT 0x40,AL |
| `0x40` | 30f5:004a | 30f5:0080 OUT 0x40,AL; 30f5:0084 OUT 0x40,AL |
| `0x43` | 30f5:0010 | 30f5:003b OUT 0x43,AL |
| `0x43` | 30f5:004a | 30f5:007b OUT 0x43,AL |

### FPU / D8-DF

- FPU mnemonic occurrences: **1**
  - 1316:0f8c `FADD float ptr [BX + DI + 0x63c3]` in 1316:0d87
- Instructions with first opcode byte D8-DF: **1**
  - 1316:0f8c byte `0xd8` `FADD float ptr [BX + DI + 0x63c3]` in 1316:0d87

### Asset-loader chain (file I/O)

Low-level DOS wrappers:

| purpose | function |
|---|---|
| open (AH=3Dh, read-only) | `31b1:000b` |
| read (AH=3Fh) | `3247:0009` |
| lseek (AH=42h; AL from caller) | `3271:0004` |
| close (AH=3Eh) | `3218:000e` |
| file length (IOCTL 44h + seek 42h) | `32ab:000a` |
| tell/file length variant (seek 42h) | `3326:0009` |
| get/set attributes (AH=43h) | `32a9:0009` |
| open+close wrapper | `3197:003e` |
| write (AH=40h) | `3197:002a`, `3268:0009`, `1000:01a7` |

Higher-level loaders: `1316:0002` (game init; calls open/read/seek/close/filelength), `1237:0904` (resource loader; also calls the LZSS decoder `1057:0000`), `1237:0113`, `31cb:00f1`, `323b:0004`, `31b6:0056`.

### Interrupt hooks

- Keyboard IRQ1 (vector 9): install `307a:0000`, query `307a:01cf`.
- Timer IRQ0 (vector 8): query `30f5:0010`, install/restore `30f5:004a`.
- Ctrl-Break (vector 0x1B): get/set `3360:0000`.
- CPU exceptions vectors 0/4/5/6: save+install `1000:012f`, restore `1000:0172`.
- `INT 10h`: text mode 3 (`1316:0002`, `1316:0365`), mode 4 + cursor (`307a:0353`), VGA/EGA detection (`306a:0037`, AH=1Ah/12h).
- `INT 33h`: only mouse reset/get-installed (`307a:01ec`, AX=0).
- `IN/OUT`: VGA status `0x3DA` (`1316:1302`), game/joystick port `0x201` (`307a:0428`), PIT `0x40/0x43` (`30f5:0010`, `30f5:004a`). No `0x388/0x389`, no Sound Blaster `2xx`, no keyboard `0x60/0x61`, no PIC `0x20/0x21`.
- FPU: only one suspicious `FADD` at `1316:0f8c` (opcode D8) in an orphan function; no `INT 34h-3Dh`, so no 8087-emulator path was found.

## 4. Turbo C runtime / helper candidates

| address | probable role | evidence |
|---|---|---|
| `1000:0000` | C startup / `main` dispatch | DOS version, memory resize, calls exit |
| `1000:010d` | termination / exit | `INT 21h AH=4Ch` |
| `1000:012f` | save old exception vectors, install handler | `INT 21h AH=35h` x4, `AH=25h` |
| `1000:0172` | restore saved exception vectors | `INT 21h AH=25h` x4 |
| `1000:01d1`, `1000:025f`, `1000:02cf` | command-line/environment/argv startup | PSP `0x80`, quote/backslash parsing, far pointer list |
| `1000:0313` | add integer to normalized far pointer | 4-bit segment normalization |
| `1000:0396` | add offset to far-pointer offset | nibble arithmetic |
| `1000:03f1` | compare normalized far pointers | compares DX:CX vs AX:BX |
| `1000:052a` | far-pointer difference | `(DX*16+AX) - (BX+CX*16)` |
| `1000:0412` | long multiply (`N_LXMUL`) | 32x32 `MUL` sequence |
| `1000:042b` / `1000:042f` | unsigned / signed long divide entries | set CX=0 / CX=1 then share body |
| `1000:0439` | long divide/modulo body (`N_LDIV`/`N_LUDIV`) | 32-iteration shift/subtract, sign/remainder flags |
| `1000:04ce` | signed long right shift (`N_LXRSH`) | `SAR`, sign handling |
| `1000:04ec` | long left shift (`N_LXLSH`) | `SHL` pair |
| `1000:050b` | unsigned long right shift (`N_LXURSH`) | `SHR` pair |
| `1000:054f` | `memcpy` / `memmove` | far pointers + `REP MOVSW`/`MOVSB` |
| `1057:0000` | LZSS resource decompressor | bit flags, literal/back-reference copy; called by `1237:0904` |
| `14b0:0451` | game text formatter (printf-like) | parses `%`, digits, calls text output; not standard libc |

No standard Turbo C `printf`/`strcpy`/`strlen` was confidently isolated. The game uses custom `%S`/`%W` format specifiers; `14b0:0451` is the best candidate for the formatter. `30d5:0000/0110/0146` are game-side buffer helpers, not confirmed libc.

## 5. Largest functions

| address | size | insns | segment |
|---|---:|---:|---|
| `2449:134e` | 5245 | 1917 | 2449 |
| `20b5:2c06` | 3396 | 1382 | 20b5 |
| `2c74:0006` | 2775 | 1127 | 2c74 |
| `2d65:13e2` | 2616 | 988 | 2d65 |
| `2b79:0003` | 2579 | 904 | 2b79 |
| `26c5:071a` | 2563 | 1062 | 26c5 |
| `20b5:085b` | 2348 | 1014 | 20b5 |
| `26c5:111d` | 2266 | 927 | 26c5 |
| `2920:026c` | 2218 | 879 | 2920 |
| `2b79:0ea5` | 1828 | 716 | 2b79 |
| `1ba8:1005` | 1771 | 796 | 1ba8 |
| `1b06:0007` | 1710 | 561 | 1b06 |
| `2920:0a12` | 1621 | 645 | 2920 |
| `2a6e:0809` | 1510 | 636 | 2a6e |
| `18cb:0002` | 1454 | 474 | 18cb |
| `2864:0412` | 1335 | 538 | 2864 |
| `105e:113b` | 1322 | 430 | 105e |
| `105e:095d` | 1321 | 445 | 105e |
| `1316:088b` | 1276 | 430 | 1316 |
| `14b0:0451` | 1238 | 424 | 14b0 |

Functions per segment (top): `1ba8` 44, `1316` 31, `15b8` 30, `105e` 30, `2449` 26, `2d65` 25, `1000` 23, `18cb` 22, `20b5` 19, `14b0` 15, `1237` 11, `2920` 10.

## 6. Game files

`game\` in the old repo is empty; the actual distribution files now live in `ghidra\`:

| file | bytes | file | bytes |
|---|---:|---|---:|
| ADLIB.DVR | 6080 | CMS.DVR | 12512 |
| COVOX.DVR | 6208 | EGA.DVR | 9296 |
| IBM.DVR | 3088 | MCGA.DVR | 5238 |
| NULL.DVR | 181 | ROL.DVR | 1664 |
| TAN.DVR | 6413 | TANDY.DVR | 3584 |
| D1.SAV / D2.SAV / D3.SAV | 9077 each | DTH.SAV / DUKE.SAV | 9077 each |
| TRY1.SAV / TRY2.SAV | 9077 each | EPISODE1-5.DAT | 9077 each |
| EXCAL.EXE | 88619 | EXCAL_UNPACKED.EXE | 188924 |
| START.EXE | 20557 | START_UNPACKED.EXE | 39634 |
| INSTALL.EXE | 13550 | INSTALL.PIF | 967 |
| GAME.CFG | 606 | SEX.IDF | 866 |
| HINT.RES | 36990 | MUSIC.RES | 9816 |
| SCENES.RES | 12804 | DISK1-DISK6 | 3 each |

Filename strings found in `strings.tsv`: `game.cfg` (`3364:0198`) and `EPISODE1.DAT`-`EPISODE5.DAT` (`3364:7acf`-`3364:7b03`). No exported function's data-reference list points directly at these addresses; only adjacent table bytes `3364:7ac6/7ac7/7ac9/7acd` are referenced (by `2d65:08a4`, `2d65:13e2`, `2d65:2cd9`). The filename use is therefore indirect/table-driven and was not resolved by Ghidra. The main open/read path is in `1316:0002`.

# Follow-up 2: switch tables, canonical branch comments, far pointers

Backup before changes: `ghidra_backup_20261020_140845.zip`.

## Switch tables

Scan result: **18** `JMP/CALL word ptr CS:[reg+imm]` instructions in the listing. **17** are in exported function bodies (15 functions); `1ba8:324e` is in an unanalysed orphan region with no containing function. **7** of the 15 exported functions are among the 50 largest.

Hypothesis verified on real examples:

| JMP | table | entry0 | N | bound | result |
|---|---|---|---:|---:|---|
| `2d65:0ae3` | `0ae8` | `0af0` | 4 | CMP AX,3 / JBE | fits; targets `0af0,0b31,0b65,0b70` |
| `1316:0298` | `029d` | `02a5` | 4 | CMP AX,3 / JA | fits |
| `20b5:0fd2` | `0fd7` | `0fef` | 12 | CMP AX,0xb / JA | fits |
| `18cb:0ead` | `0eb2` | `0ec2` | 8 | CMP AX,7 / JBE | fits |
| `2920:0437` | `043c` | `044c` | 8 | CMP AX,7 / JA | fits |
| `2b79:0f55` | `0f5a` | `0f86` | 22 | CMP AX,0x15 / JA | fits |

Non-fitting / skipped cases:

- `14b0:08f1`, `18cb:0ca8`, `18cb:0fac`, `18cb:10cf`, `18cb:121f`, `18cb:13e2`, `20b5:19ff` — these are the linear-search pattern (`CMP AX,CS:[BX]`, `LOOP`), N computes negative; not switch tables.
- `1ba8:324e` — N=603 from entry0 `3709`, but preceding `CMP AX,0x41` implies only 66 cases. It is in an unanalysed region (no containing function); not fixed.
- `2d65:1dd9` — N=85 from entry0 `1e88`, but `CMP AX,0x36` implies only 55 cases; extra table entries continue past the switch bound. Not fixed.

Resolved **9** switches, skipped **9**. `ResolveCsSwitchTables.java` creates the word table, removes stale computed-jump refs, adds correct `COMPUTED_JUMP` refs, disassembles targets, writes a `JumpTable` override, and calls `CreateFunctionCmd.fixupFunctionBody`.

### Function size changes (before → after)

| function | size | insns |
|---|---|---|
| `1316:0002` | 983 → 859 | 332 → 287 |
| `1316:0d87` | 608 → 758 | 221 → 279 |
| `18cb:0e5f` | 340 → 570 | 131 → 213 |
| `20b5:085b` | 2348 → 2020 | 1014 → 876 |
| `20b5:1dff` | 205 → 3103 | 79 → 1144 |
| `2920:026c` | 2218 → 1063 | 879 → 419 |
| `2b79:0ea5` | 1828 → 229 | 716 → 87 |
| `2d65:0ac8` | 36 → 248 | 15 → 101 |
| `2d65:0bc8` | 72 → 345 | 29 → 142 |

**9 functions changed size.** Some shrank because the first pass had stale computed-jump refs to other segments; the rerun removed those refs and recomputed the body correctly.

Decompile failures: **4 before → 3 after**. `2d65:0bc8` now decompiles. Remaining: `1237:0d62`, `1ba8:2e27`, `2d65:13e2`.

`ExportForAI.java` now appends branch comments to the existing disassembly text, e.g.:

```text
2d65:0ace  ...  CALLF 0x1000:7c98  ; call -> FUN_1772_0578 @ 1772:0578
2d65:0ada  ...  JBE 0x2000:e12f    ; jump -> 2d65:0adf
2d65:0ae3  ...  JMP word ptr CS:[BX + 0xae8]  ; jump -> 2d65:0af0, 2d65:0b31, 2d65:0b65, 2d65:0b70
```

## DGROUP far-pointer tables

Scan covered `3364:0000`-`3364:81bb`, every byte offset, exact function entries only.

- **20 valid 4-byte far pointers**
- **2 contiguous tables** plus **1 isolated pointer**
- Unique functions pointed to: **18**
- Zero-caller functions: **108** now (113 before switch fix); over 300 bytes: **25** (27 before)
- Zero-caller functions covered by these pointers: **15** (14 in `18cb` plus `entry`)
- Still unexplained: **93**

Tables:

- DS `0x0389`, **16 entries**, 15 unique functions, all in segment `18cb` (`08ff`, `0944`, `0b06`, `0bb1`, `0bdb`, `0c2d`, `0c8a`, `0de6`, `0e5f` twice, `15c3`, `15d3`, `1201`, `13c4`, `15e3`, `15ea`). This is the main dispatch/callback table.
- DS `0x79a6`, **3 entries**: `1772:0578`, `1772:0578`, `2920:1067`. Both targets already have direct callers.
- DS `0x06c9`, isolated pointer to `1000:0000` (`entry`).

Full table and the 93 unexplained zero-caller functions are in `notes/FUNC_POINTERS.md`.

# Follow-up 3: sparse switches, coverage, gaps, relocations

Backup before changes: `ghidra_backup_20261020_142008.zip`.

## Sparse switches

The sparse hypothesis held on all 7 true sparse switches. Example raw tables:

- `14b0:08f1`: `MOV CX,7`, value table `0x8f5`, `JMP [BX+0xe]`; values `0052 0053 0057 0063 0064 006c 0073`, targets `14b0:0911 0917 0911 0911 0911 0917 0917`.
- `18cb:0fac`: `MOV CX,8`, value table `0xfb0`, `JMP [BX+0x10]`; targets `0fd0 0fec 1008 1024 1040 105b 1076 1095`.
- `20b5:19ff`: `MOV CX,8`, value table `0x1a03`, `JMP [BX+0x10]`; values are bit masks `0000 0001 0002 0004 0008 0010 0020 0040`.

`ResolveCsSwitchTables.java` now handles both forms. Final run: **18 found, 11 dense, 7 sparse, 0 skipped**.

- All 7 sparse switches resolved: `14b0:08f1`, `18cb:0ca8/0fac/10cf/121f/13e2`, `20b5:19ff`.
- `1ba8:324e` is **not** sparse. It is dense with `CMP AX,0x41`; resolved with N=66 as an orphan table (no containing function).
- `2d65:1dd9` is dense; the CMP bound gives N=55, which was preferred over the entry0 formula (85), and resolved.

Seven functions changed size in this pass:

| function | before | after |
|---|---:|---:|
| `14b0:0451` | 1238 | 1250 |
| `18cb:0c8a` | 40 | 256 |
| `18cb:0e5f` | 570 | 866 |
| `18cb:1201` | 40 | 431 |
| `18cb:13c4` | 40 | 495 |
| `20b5:1705` | 815 | 1754 |
| `2d65:13e2` | 2616 | 2976 |

Decompile failures: **3 before → 2 after**. Remaining: `1237:0d62`, `1ba8:2e27`. `2d65:13e2` now decompiles.

## Coverage and gaps

`notes/COVERAGE.md` has the full per-block table. Totals across 86 code blocks:

- block bytes: **144,960**
- covered by functions: **132,267** (91.2%)
- covered by recognised switch tables: **576**
- gaps >=16 bytes: **28**, totalling **11,941** bytes
- overlapping function pairs: **0**

Special cases:

- `20b5:1dff` 205 → 3103: added its own dense-switch case blocks (`20b5:1ec1`, N=9) up to `20b5:2a1e`. Next function starts at `20b5:2a30`; no overlap, so it did **not** swallow neighbours.
- `2b79:0ea5` 1828 → 229: earlier body was inflated by stale computed-jump refs to unrelated segments; current body is `2b79:0ea5`–`2b79:0f89`.
- `2920:026c` 2218 → 1063: current body ends at `2920:0693`, just before `FUN_2920_06a3`. The old range would have overlapped several following functions; those are now separate, with **0 overlaps**.

Gap prologue scan (`55 8B EC`) found **7 candidates**; no functions were created. See `notes/GAPS.md`:

`2036:0009` (gap 1132), `2a6e:0009` (55), `2d65:0001` (2211), `321b:0004` (56), `321e:000c` (58), `332f:000c` (37), `3337:0007` (33).

## Relocations and zero-caller functions

MZ relocation table: **2598** relocations; **2381** segment words point at real code segments.

- **2364** unique function-pointer pairs found; **319** unique target functions.
- Data pointers: **79** — **2 tables** and **60 isolated**.
  - `3364:0389`: 16 entries, 15 unique `18cb` functions (the dispatch table).
  - `3364:79a6`: 3 entries (`1772:0578` twice, `2920:1067`).
- Code pointers: **2228** direct CALLF/immediate references and **57** MOV/PUSH immediate pairs; individual, not tables. Full list in `notes/reloc_pointers_raw.tsv`.

Zero-caller function count is now **107**. Relocation pointers explain **40** of them; **67 remain unexplained**. The DGROUP-only scan explained 15; relocations add 25 more. Full covered/unexplained lists and isolated data pointers are in `notes/FUNC_POINTERS.md`.

Oddities: segment `1ba8` still has the largest unexplained gaps (`5194` bytes across 7 gaps) and none of its large gaps start with `55 8B EC`; `1ba8:324e` is a 66-case switch in that unanalysed region. The relocation scan is dominated by direct CALLF references (2228); only 57 are MOV/PUSH pointer pairs.

# Follow-up 5: gap recovery

Backup before changes: `ghidra_backup_20261020_144003.zip`.

## Functions created

- 7 prologue candidates: `2036:0009`, `2a6e:0009`, `2d65:0001`, `321b:0004`, `321e:000c`, `332f:000c`, `3337:0007`.
- Large gaps: `2036:0009` became a single 1132-byte function; `2d65:0001` a single 2211-byte function. No additional `55 8B EC` prologue or `RETF`-separated function was found, so no split was needed.
- Reference recovery found 239 candidates inside the original 28 gaps; 15 became functions (14 listed in `notes/gap_recovery_raw.tsv` plus `FUN_1ba8_3032`). The orphan 66-case switch at `1ba8:324e` belongs to **`FUN_1ba8_3032`** (4378 bytes). A provisional 1-byte `1ba8:3030` function was a mid-instruction artifact and was removed.
- Total functions: **461 -> 482** (21 new).

## Data classification

Only clear data regions were marked (strings, far-pointer runs, or padding); code-like gaps were left alone. **8 regions / 2477 bytes** marked as data; see `notes/classified_data_raw.tsv`.

## Final state

- coverage: **144,654 / 144,960 bytes = 99.79%**
- remaining gaps >=16 bytes: **4**, totalling **133 bytes**:
  `1000:03c3` (16), `1ba8:30b2` (34), `2b4c:0212` (44), `2b4c:025f` (39)
- zero-caller functions: **123** (31 over 300 bytes)
- decompile failures: **2** (`1237:0d62`, `1ba8:2e27`)
- overlapping function pairs: **0**

`notes/COVERAGE.md` has the regenerated per-block table; `notes/GAPS.md` has the recovery log and remaining gaps.



