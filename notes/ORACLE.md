# Oracle status

Status: **working for pure memory routines**, not yet for DOS/BIOS calls.

- `tests/oracle/excal_oracle.py` loads `EXCAL_UNPACKED.EXE`'s payload at
  `1000:0000` (linear `0x10000`), so Ghidra addresses are runtime addresses.
- Real-mode Unicorn, DS/ES/SS and a far-return sentinel are configured.
- Verified LZSS (`1057:0000`) and secondary RLE (`30d5:0110`).
- Vectors: `tests/vectors/decompressors.tsv` (git-ignored), fields
  `flag, file, id, input_hex, output_hex`.
- Replay test: `soe-res` `oracle_lzss_vectors` (skips if vectors absent).

## Decompressor results

- **523/523 flag-0x80 LZSS records match the original byte-for-byte.**
- **20 flag-0x01 RLE records** use the verified token layout:
  `0x80` no-op, `0x00..=0x7F` literal run `token+1`,
  `0x81..=0xFF` repeat next byte `0x101-token` times.
- 15 RLE records differ only in bytes after the declared record tail: the
  original keeps reading the next in-memory bytes while our implementation
  zero-pads. This is an input-boundary difference, not a token-layout mismatch.
  The canonical case `LOGO.RES#10057` matches exactly (40,006 bytes).

## Not covered yet

The oracle does not emulate DOS interrupts, video ports or the MCGA driver.
Those need adding before scene/text/save code can be oracle-tested.
