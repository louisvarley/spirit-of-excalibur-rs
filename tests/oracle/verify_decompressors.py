#!/usr/bin/env python3
"""Compare soe-res decompressors with the original routines via Unicorn."""
from __future__ import annotations

import struct
from collections import Counter, defaultdict
from pathlib import Path

from excal_oracle import Oracle

ROOT = Path(__file__).resolve().parents[2]
GAME = ROOT / "game"
VECTORS = ROOT / "tests" / "vectors" / "decompressors.tsv"
MAX = 16 * 1024 * 1024


def lzss(data: bytes, expanded: int) -> bytes:
    out = bytearray(); i = 0
    while len(out) < expanded and i < len(data):
        flags = data[i]; i += 1
        for bit in range(8):
            if len(out) >= expanded or i >= len(data):
                break
            if flags & (1 << bit):
                out.append(data[i]); i += 1
            else:
                if i + 1 >= len(data):
                    break
                b, c = data[i], data[i + 1]; i += 2
                length = (b & 0x3F) + 3
                off = ((b >> 6) & 3) | (c << 2)
                if off == 0:
                    return bytes(out)
                if off > len(out):
                    return bytes(out)
                for _ in range(length):
                    out.append(out[-off])
    return bytes(out)


def rle(data: bytes, cap: int = 0x10000) -> bytes:
    out = bytearray(); i = 0
    while i < len(data) and len(out) < cap:
        token = data[i]; i += 1
        if token == 0x80:
            continue
        if token <= 0x7F:
            n = token + 1
            out += data[i:i + n]; i += n
        else:
            n = 0x101 - token
            if i >= len(data):
                break
            out += bytes([data[i]]) * n; i += 1
    return bytes(out)


def records(path: Path):
    d = path.read_bytes()
    _, io, _, il = struct.unpack_from("<IIII", d, 0)
    d4, d5 = struct.unpack_from("<II", d, 0x10)
    base = 0x18 if (d4 and d4 < MAX and d5 >= d4 + 4 and d5 < MAX) else 0x14
    best = []
    for start in range(io + 16, io + il - 12, 2):
        recs = []; p = start
        while p + 12 <= io + il and d[p + 2:p + 4] == b"\xff\xff":
            rid = struct.unpack_from("<H", d, p)[0]
            off = d[p + 4] | d[p + 5] << 8 | d[p + 6] << 16
            flags = d[p + 7]
            recs.append((rid, off, flags))
            p += 12
        if len(recs) > len(best):
            best = recs
    out = []
    for i, (rid, off, flags) in enumerate(best):
        nxt = best[i + 1][1] if i + 1 < len(best) else io - base
        out.append((rid, flags, d[base + off:base + nxt], base + off))
    return out


def main():
    oracle = Oracle()
    VECTORS.parent.mkdir(parents=True, exist_ok=True)
    counts = Counter(); per_file = defaultdict(Counter)
    mismatches = []; tested = 0; vectors = []
    files = sorted(GAME.glob("*.RES")) + sorted((GAME / "RES32").glob("*.RES"))
    for path in files:
        for rid, flags, stored, start in records(path):
            per_file[path.name][flags] += 1
            counts[flags] += 1
            if flags & 0x80:
                expanded = struct.unpack_from("<I", path.read_bytes(), start - 4)[0]
                if expanded <= 0 or expanded >= MAX:
                    mismatches.append((path.name, rid, "bad expanded size"))
                    continue
                original = oracle.lzss(stored, expanded)
                ours = lzss(stored, expanded)
            elif flags == 0x01:
                original = oracle.rle(stored)
                ours = rle(stored)
            else:
                continue
            tested += 1
            if original != ours:
                mismatches.append((path.name, rid, f"{len(original)} vs {len(ours)}"))
            vectors.append((flags, path.name, rid, stored.hex(), original.hex()))
    VECTORS.write_text("\n".join(
        f"{f}\t{n}\t{i}\t{s}\t{o}" for f, n, i, s, o in vectors
    ) + "\n")
    print("records", sum(counts.values()), "flags", dict(counts))
    print("tested", tested, "mismatches", len(mismatches))
    for name, c in sorted(per_file.items()):
        print(name, dict(c))
    for m in mismatches[:20]:
        print("MISMATCH", m)


if __name__ == "__main__":
    main()
