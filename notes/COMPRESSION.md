# Compression and `.RES` layout

Verified from the original data and the decompressor at `1057:0000`; no original
bytes are stored here.

## LZSS stream (`1057:0000`)

Token stream:

1. Read one flag byte; consume its bits least-significant first.
2. Flag bit = 1: copy one literal byte from the input.
3. Flag bit = 0: read two bytes `b`, `c`.
   - `length = (b & 0x3f) + 3`
   - `offset = ((b >> 6) & 3) | (c << 2)` (12-bit offset)
   - `offset == 0` ends the stream.
   - Otherwise copy `length` bytes from `output.len() - offset`.
4. Stop when the declared expanded size is reached.

Verified examples:

- `MUSIC.RES` record 0 decompresses to a stream beginning `FORM`; the declared
  expanded size is 1216 bytes.
- `HINT.RES` record 0 decompresses to readable text beginning `Acclaimed by the
  remaining knights...`; declared expanded size 266.

The Rust implementation is `soe_res::LzssDecompressor`. It is behind the
`Decompressor` trait so a different codec can be supplied for unknown resources.

## `.RES` container

Header (16 bytes, little-endian):

| field | value |
|---|---|
| `data_start` | `0x10` |
| `index_offset` | offset of the index region |
| `data_length` | `index_offset - 0x10` |
| `index_length` | length of the index region |

`file_len = index_offset + index_length`. This holds for every `.RES` file in
`game/` and `game/RES32/`.

### Records

The common record form begins with two u32 values (apparently stored and
expanded size), followed by the LZSS payload. In practice the directory layout
is not fully uniform:

- `HINT.RES` and `SCENES.RES` can be walked end-to-end with a mixed
  eight-byte/ four-byte length-prefix scan.
- `MUSIC.RES`, `TITLE.RES` and several image containers stop at the first
  implausible boundary. Their directory/index layout still needs reversing.
- `SCENES.RES` record 0 does not fit the two-u32 form; its second word is
  `0xF41C0E10`, not a plausible expanded size, so it is treated as a raw
  length-prefixed record.
- The index region starts with a 16-byte header copy and appears to contain
  12-byte directory entries (`directory_entries = (index_length - 16) / 12`).
  TITLE.RES has an 8-byte remainder, so this count is labelled inferred.

Open question: the exact index/directory schema and the record header variants.
Until that is reversed, `ResFile::records()` is explicitly best-effort and the
CLI labels it as such.




## Verified directory schema (reconciled with prior repo)

Header: 4 dwords (`0x10`, directory_offset, directory_offset-0x10, directory_size)
plus either a 5-dword (payload base `0x14`, SCENES) or 6-dword (base `0x18`,
extra dwords = first stored size - 4 and first decompressed size) variant.

At `directory_offset`: 16-byte header copy, then a 22-byte type header
(`u32 0, u16 1, u16 checksum, u16 0x1C, u32 total, char type_id[4] reversed,
u16 count, u16 10`). Records are 12 bytes, first record at directory+`0x26`:

| field | type | meaning |
|---|---|---|
| 0 | u16 | resource id |
| 2 | u16 | `0xFFFF` marker |
| 4 | u24 LE | offset relative to payload base |
| 7 | u8 | flags: `0x80` LZSS, `0x00` raw, `0x01` RLE |
| 8 | u32 | type-specific extra |

Resource end = next larger offset, or directory offset for the last resource.
TITLE/INTF have several type sections; the longest consistent 12-byte run is
used. This reproduces the prior test vectors exactly.

## Audit (verified schema)

**841 records**: 523 LZSS (0x80), 298 raw (0x00), 20 secondary RLE (0x01).
All 523 LZSS records decode, and all 298 raw records are stored consistently.
The 20 flag-0x01 records are listed in `notes/IMAGE_FORMAT.md`; their RLE codec
is verified (see below).

## Flag-0x01 codec (verified from `30d5:0110`)

The loader tests bit 1 and calls `FUN_30d5_0110`. Token stream:

- `0x80`: no-op/skip.
- `0x00..=0x7F`: copy the next `token + 1` literal bytes.
- `0x81..=0xFF`: repeat the next byte `0x101 - token` times.

Decoding stops at end of input (or the output cap). This decodes LOGO/EXTER
flag-0x01 images to headers `00 c8 01 40 00 05` and ANI frames to their
`0x05 ...` frame-bank payloads. `soe_res::rle_decompress` implements it.
