#!/usr/bin/env python3
import os, csv
from collections import defaultdict

HERE = os.path.dirname(__file__)

def rows(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as fh:
        return list(csv.DictReader(fh, delimiter="\t"))

def hexv(v):
    try:
        return hex(int(v))
    except Exception:
        return "?"

AH = {
    0x09: "print $-string", 0x0e: "select disk", 0x1a: "set DTA",
    0x1c: "get drive data", 0x25: "set interrupt vector", 0x30: "DOS version",
    0x35: "get interrupt vector", 0x36: "get free space", 0x3c: "create file",
    0x3d: "open file", 0x3e: "close file", 0x3f: "read file",
    0x40: "write file", 0x41: "delete file", 0x42: "lseek",
    0x43: "get/set file attributes", 0x44: "IOCTL", 0x4a: "resize memory",
    0x4c: "exit", 0x4e: "find first", 0x4f: "find next",
}

print("### INT 21h\n")
print("| AH | meaning | calls | function entry (call addresses) |")
print("|---|---|---:|---|")
g = defaultdict(list)
for r in rows("int21.tsv"):
    g[int(r["ah"])].append(r)
for ah in sorted(g):
    items = g[ah]
    funcs = defaultdict(list)
    for r in items:
        funcs[r["entry"]].append(r["addr"])
    desc = ",".join(f"{e} ({', '.join(a)})" for e, a in funcs.items())
    print(f"| `{hexv(ah)}` | {AH.get(ah,'')} | {len(items)} | {desc} |")

print("\n### INT 10h\n")
print("| AX/AH | function entry | call address |")
print("|---|---|---|")
for r in rows("int10.tsv"):
    v = hexv(r["ax"]) if r["ax"] not in ("", "None") else "AH=" + hexv(r["ah"])
    print(f"| `{v}` | {r['entry']} | {r['addr']} |")

print("\n### INT 33h\n")
print("| AX | function entry | call address |")
print("|---|---|---|")
for r in rows("int33.tsv"):
    print(f"| `{hexv(r['ax'])}` | {r['entry']} | {r['addr']} |")

print("\n### IN / OUT ports\n")
print("| port | function entry | instructions (address: mnemonic) |")
print("|---|---|---|")
p = defaultdict(lambda: defaultdict(list))
for r in rows("io_ports.tsv"):
    port = hexv(r["port"]) if r["known"] == "True" else "DX?"
    p[port][r["entry"]].append(f"{r['addr']} {r['text']}")
for port in sorted(p, key=lambda x: (x == "DX?", x)):
    for e, ins in p[port].items():
        print(f"| `{port}` | {e} | {'; '.join(ins)} |")

print("\n### FPU / D8-DF\n")
f = rows("fpu.tsv")
o = rows("opcodes_d8_df.tsv")
print(f"- FPU mnemonic occurrences: **{len(f)}**")
for r in f:
    print(f"  - {r['addr']} `{r['text']}` in {r['entry']}")
print(f"- Instructions with first opcode byte D8-DF: **{len(o)}**")
for r in o:
    print(f"  - {r['addr']} byte `{hex(int(r['byte']))}` `{r['text']}` in {r['entry']}")
