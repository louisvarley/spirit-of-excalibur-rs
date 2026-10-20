#!/usr/bin/env python3
"""Parse the ExportForAI markdown output for I/O, FPU and runtime clues."""
import glob
import os
import re
import json
from collections import Counter, defaultdict

EXP = os.path.join(os.path.dirname(__file__), "..", "export")
FUNC_RE = re.compile(r"^# (.+) @ ([0-9A-Fa-f]{4}:[0-9A-Fa-f]{4})")
INS_RE = re.compile(r"^([0-9A-Fa-f]{4}:[0-9A-Fa-f]{4})\s+([0-9a-fA-F?]+)\s+(.*)$")

FPU = {
    "FLD", "FST", "FSTP", "FILD", "FIST", "FISTP", "FBLD", "FBSTP",
    "FADD", "FADDP", "FIADD", "FSUB", "FSUBP", "FISUB", "FSUBR", "FSUBRP",
    "FMUL", "FMULP", "FIMUL", "FDIV", "FDIVP", "FIDIV", "FDIVR", "FDIVRP",
    "FCOM", "FCOMP", "FCOMPP", "FCOMI", "FCOMIP", "FUCOM", "FUCOMP", "FUCOMPP",
    "FXCH", "FABS", "FCHS", "FSQRT", "FSIN", "FCOS", "FSINCOS", "FPTAN",
    "FPATAN", "F2XM1", "FYL2X", "FYL2XP1", "FSCALE", "FPREM", "FPREM1",
    "FRNDINT", "FXTRACT", "FLDCW", "FSTCW", "FNSTCW", "FLDENV", "FSTENV",
    "FNSTENV", "FSAVE", "FNSAVE", "FRSTOR", "FINIT", "FNINIT", "FWAIT",
    "WAIT", "FFREE", "FNOP", "FDECSTP", "FINCSTP", "FTST", "FXAM", "FSETPM",
    "FLDL2E", "FLDL2T", "FLDLG2", "FLDLN2", "FLDPI", "FLDZ", "FLD1",
}
MATH = {"IMUL", "MUL", "IDIV", "DIV", "SHL", "SHR", "SAR", "SAL", "RCL",
        "RCR", "ADC", "SBB", "CWD", "CDQ", "CBW", "NEG"}
STRINGOPS = {"MOVSB", "MOVSW", "STOSB", "STOSW", "LODSB", "LODSW",
             "SCASB", "SCASW", "CMPSB", "CMPSW"}


def load_functions():
    funcs = []
    for path in sorted(glob.glob(os.path.join(EXP, "functions", "*.md"))):
        name = entry = None
        insns = []
        data = []
        in_dis = False
        in_dat = False
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                m = FUNC_RE.match(line)
                if m and name is None:
                    name, entry = m.group(1), m.group(2)
                    continue
                if line.startswith("## Referenced data and strings"):
                    in_dat, in_dis = True, False
                    continue
                if line.startswith("## Decompiled"):
                    in_dat, in_dis = False, False
                    continue
                if line.startswith("## Disassembly"):
                    in_dat, in_dis = False, True
                    continue
                if line.startswith("```"):
                    continue
                if in_dat and line.startswith("- "):
                    data.append(line[2:].strip())
                if in_dis:
                    m = INS_RE.match(line)
                    if m:
                        insns.append((m.group(1), m.group(2), m.group(3).strip()))
        funcs.append({"name": name, "entry": entry, "insns": insns, "data": data})
    return funcs


def imm(s):
    m = re.search(r"0x([0-9A-Fa-f]+)", s)
    return int(m.group(1), 16) if m else None


def split_ops(text):
    # split first operand only (enough for IN/OUT)
    p = text.find(",")
    if p < 0:
        return text.strip(), ""
    return text[:p].strip(), text[p + 1:].strip()


def analyze(f):
    ax = ah = al = dx = bx = cx = None
    int21, int10, int33, ports, fpu, opcodes = [], [], [], [], [], []
    math_hits = Counter()
    str_hits = Counter()
    for addr, byts, text in f["insns"]:
        up = text.upper()
        mnem = up.split()[0] if up.split() else ""
        # decode constants before handling interrupts
        rest = text[len(mnem):].strip()
        if mnem == "MOV":
            dst, src = split_ops(rest)
            d = dst.upper().replace(" ", "")
            sv = imm(src)
            if d == "AX":
                ax = sv; ah = (sv >> 8) & 0xFF if sv is not None else None
                al = sv & 0xFF if sv is not None else None
            elif d == "AH":
                ah = sv
                if ah is not None and al is not None:
                    ax = (ah << 8) | al
                else:
                    ax = None
            elif d == "AL":
                al = sv
                if ah is not None and al is not None:
                    ax = (ah << 8) | al
                else:
                    ax = None
            elif d == "DX":
                dx = sv
            elif d == "BX":
                bx = sv
            elif d == "CX":
                cx = sv
            elif d in ("AX", "AL", "AH", "DX", "BX", "CX"):
                pass
            else:
                # memory destination: invalidate only if it writes through a
                # register we track
                if "[" in d:
                    pass
        elif mnem in ("XOR", "SUB") and "," in rest:
            a, b = [x.strip().upper() for x in rest.split(",", 1)]
            if a == b and a in ("AX", "AH", "AL", "DX", "BX", "CX"):
                if a == "AX":
                    ax = ah = al = 0
                elif a == "AH":
                    ah = 0
                elif a == "AL":
                    al = 0
                elif a == "DX":
                    dx = 0
                elif a == "BX":
                    bx = 0
                elif a == "CX":
                    cx = 0
        elif mnem in ("INC", "DEC"):
            d = rest.upper()
            v = {"AX": ax, "AH": ah, "AL": al, "DX": dx, "BX": bx, "CX": cx}.get(d)
            if v is not None:
                v = (v + (1 if mnem == "INC" else -1)) & 0xFFFF
                if d == "AX":
                    ax = v; ah = v >> 8; al = v & 0xFF
                elif d == "AH":
                    ah = v & 0xFF; ax = None
                elif d == "AL":
                    al = v & 0xFF; ax = None
                elif d == "DX":
                    dx = v
                elif d == "BX":
                    bx = v
                elif d == "CX":
                    cx = v
        elif mnem in ("ADD", "SUB") and "," in rest:
            d, s = [x.strip().upper() for x in rest.split(",", 1)]
            sv = imm(s)
            if sv is not None and d in ("AX", "DX", "BX", "CX"):
                cur = {"AX": ax, "DX": dx, "BX": bx, "CX": cx}[d]
                if cur is not None:
                    nv = (cur + (sv if mnem == "ADD" else -sv)) & 0xFFFF
                    if d == "AX":
                        ax = nv; ah = nv >> 8; al = nv & 0xFF
                    elif d == "DX":
                        dx = nv
                    elif d == "BX":
                        bx = nv
                    elif d == "CX":
                        cx = nv

        if mnem == "INT":
            vec = imm(text)
            if vec == 0x21:
                int21.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                              "ah": ah, "al": al, "ax": ax, "text": text})
            elif vec == 0x10:
                int10.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                              "ah": ah, "al": al, "ax": ax, "text": text})
            elif vec == 0x33:
                int33.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                              "ah": ah, "al": al, "ax": ax, "text": text})
            # interrupts may clobber registers
            ax = ah = al = dx = bx = cx = None
            continue

        if mnem in ("IN", "OUT"):
            op1, op2 = split_ops(rest)
            port_s = op2 if mnem == "IN" else op1
            p = imm(port_s)
            p_known = p is not None
            if p is None and "DX" in port_s.upper():
                p = dx
                p_known = p is not None
            ports.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                          "mnem": mnem, "port": p, "known": p_known, "text": text})

        if mnem in FPU:
            fpu.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                        "text": text})
        try:
            b0 = int(byts[:2], 16)
            if 0xD8 <= b0 <= 0xDF:
                opcodes.append({"func": f["name"], "entry": f["entry"], "addr": addr,
                                "byte": b0, "text": text})
        except ValueError:
            pass

        if mnem in MATH:
            math_hits[mnem] += 1
        if mnem in STRINGOPS or "REP " in up:
            for s in STRINGOPS:
                if s in up:
                    str_hits[s] += 1

        if mnem in ("CALL", "CALLF"):
            ax = ah = al = dx = bx = cx = None

    return {"int21": int21, "int10": int10, "int33": int33, "ports": ports,
            "fpu": fpu, "opcodes": opcodes, "math": math_hits, "strops": str_hits}


def fmt_int(rows, key, label):
    c = Counter((r[key] if r[key] is not None else -1) for r in rows)
    return ", ".join(f"{'?' if k < 0 else hex(k)}:{v}" for k, v in sorted(c.items()))


def main():
    funcs = load_functions()
    allr = {k: [] for k in ("int21", "int10", "int33", "ports", "fpu", "opcodes")}
    math = defaultdict(Counter)
    strops = defaultdict(Counter)
    for f in funcs:
        r = analyze(f)
        for k in allr:
            allr[k].extend(r[k])
        for k, v in r["math"].items():
            math[f["entry"]][k] += v
        for k, v in r["strops"].items():
            strops[f["entry"]][k] += v

    print("functions", len(funcs))
    print("INT21", len(allr["int21"]), fmt_int(allr["int21"], "ah", "AH"))
    print("INT10", len(allr["int10"]), fmt_int(allr["int10"], "ah", "AH"))
    print("INT33", len(allr["int33"]), fmt_int(allr["int33"], "ax", "AX"))
    print("IN/OUT", len(allr["ports"]))
    print("FPU", len(allr["fpu"]), "D8-DF opcodes", len(allr["opcodes"]))

    with open(os.path.join(os.path.dirname(__file__), "io_results.json"), "w", encoding="utf-8") as fh:
        json.dump({"summary": {
            "functions": len(funcs),
            "int21": len(allr["int21"]), "int10": len(allr["int10"]),
            "int33": len(allr["int33"]), "ports": len(allr["ports"]),
            "fpu": len(allr["fpu"]), "opcodes": len(allr["opcodes"]),
        }, **allr}, fh, indent=1)

    def dump(name, rows):
        with open(os.path.join(os.path.dirname(__file__), name), "w", encoding="utf-8") as fh:
            fh.write("\t".join(rows[0].keys()) + "\n" if rows else "")
            for r in rows:
                fh.write("\t".join(str(r[k]) for k in r) + "\n")

    dump("int21.tsv", allr["int21"])
    dump("int10.tsv", allr["int10"])
    dump("int33.tsv", allr["int33"])
    dump("io_ports.tsv", allr["ports"])
    dump("fpu.tsv", allr["fpu"])
    dump("opcodes_d8_df.tsv", allr["opcodes"])

    with open(os.path.join(os.path.dirname(__file__), "runtime_candidates.tsv"), "w", encoding="utf-8") as fh:
        fh.write("entry\tmath_ops\tstring_ops\n")
        for e in sorted(set(math) | set(strops)):
            fh.write(e + "\t" + ",".join(f"{k}={v}" for k, v in math[e].most_common())
                     + "\t" + ",".join(f"{k}={v}" for k, v in strops[e].most_common()) + "\n")


if __name__ == "__main__":
    main()
