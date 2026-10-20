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
