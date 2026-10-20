# Image and palette format

Status: **partly verified, not sufficient for a PNG exporter yet**.

## Evidence

`MCGA.DVR` was imported and analysed. Its blit routines read a pointer to an
image header and use:

- word at `+2` as a width-like field;
- word at `+4` as a bit-depth/type field (8/16/32 paths);
- a 6-byte-per-type subtype table indexed by a caller-supplied image type;
- clipping and a source pixel `0` transparency test in the blit path.

Decompressed RES32 records provide size evidence for a 6-byte header:

| file | expanded size | candidate dimensions | size check |
|---|---:|---|---|
| `RES32/EXTER.RES` record 0 | 20006 | 100 x 200 + 6 | exact |
| `RES32/ENGLAND.RES` record 0 | 40006 | 200 x 200 + 6 | exact |
| `RES32/URBAN.RES` record 0 | 13756 | 125 x 110 + 6 | exact |
| `RES32/TITLE.RES` record 0 | 116 | 11 x 10 + 6 | exact |

The leading bytes of these records look like `00 <width> ... 00 05` (e.g.
`00 64 01 40 00 05` for EXTER, `00 c8 01 40 00 05` for ENGLAND,
`00 7d 00 a7 00 05` for URBAN, `00 0b 00 0d 00 05` for TITLE). The first word
appears to be a big-endian width; the second word is **not** simply height
(100->320, 125->167, 200->320), so it is probably a stride/flags/bit-depth
field. The dimensions above are inferred from `expanded_size - 6`; they are
**unverified**.

The payload is not raw 8-bit pixels: the driver has planar/RLE/masked decode
paths and bit depths 8/16/32. Therefore no `soe img dump` was added. A PNG
exporter needs the actual pixel decoder, not just the record header.

## Record types

RES32 records are mixed. First records are sometimes palette/header data
(`TITLE` 116 bytes, `INTF` 4320 bytes) and sometimes image-like payloads.
Image-like candidates are identified by a 6-byte header plus a payload whose
size matches a plausible width x height, and by the `00 ?? ?? ?? 00 05`
prefix. Animation/script/map records do not follow that size pattern.

## Palettes

- Palette operations are on `MCGA.DVR` entries 7/8 and helpers `0963`/`09c3`.
- DAC writes use `3C8/3C9`; reads use `3C7/3C9`; VGA status is polled on `3DA`.
- The driver routines use a **0x60-byte (32 RGB entry) buffer**, with fades
  stepping components by ±2. Whether a separate full 256-colour palette is
  loaded elsewhere is unverified.
- Palette source: not yet proven to be a dedicated record type or embedded in
  images. Association between an image and its palette is unverified.

## Transparency, sprites, scenes, text

- Transparency: palette index **0** is treated as transparent in the blit path
  (verified in the driver code); XOR mode also exists.
- Sprites/sheets: unverified. The header/subtype table suggests several image
  layouts, possibly sprite sheets or animation strips.
- Scene composition: unverified; no evidence yet for a tile engine versus
  full-screen painted scenes.
- Palette cycling/swaps: the presence of palette fade/update entries suggests
  runtime palette effects, but which images use them is unverified.
- Font: text entry exists; font data/glyph format is not reversed.

## Impact on later upscaling

- The image payload decoder must be implemented before bulk PNG export.
- Preserve indexed data and palette effects; do not assume RGBA originals.
- Transparency index 0 and XOR/mask modes must survive upscaling.
- Scene composition/tile edges and sprite-sheet frame layout are still unknown,
  so an upscaler cannot yet know which pixels belong to one sprite.


## Diagnostic PNG attempt

Decompressed record 0 of four RES32 files was written to git-ignored
`out/img/` as a diagnostic 8-bit grayscale PNG, skipping the 6-byte header:

- `EXTER_rec0_100x200.png` (20006 bytes, 100x200)
- `ENGLAND_rec0_200x200.png` (40006 bytes, 200x200)
- `URBAN_rec0_125x110.png` (13756 bytes, 125x110)
- `TITLE_rec0_11x10.png` (116 bytes, 11x10)

The first three use all 256 byte values and have entropy 4.26/4.83/5.62; an
ASCII downsample shows large structured regions, not noise. That supports the
idea that the payload after the 6-byte header is image-like, but the palette,
row order, and exact header semantics are not verified. TITLE's 116-byte
record has only 9 values and is likely palette/header data, not a picture.

`soe img dump` was **not** added because a wrong decoder would silently produce
garbage. The diagnostic PNGs are not committed and are not an asset pipeline.


## Verified image format (corrected)

Header is **3 big-endian u16: height, width, planes**. Body: for each row, each
plane's row in order; stride = `ceil(width/8)` rounded up to even; MSB first;
index = `sum(bit(plane) << plane)`. Expected size =
`6 + planes * height * even(ceil(width/8))`.

This corrects the earlier assumption that the first word was bytes/scanline:

| resource | header | dimensions | size |
|---|---|---|---|
| ENGLAND 510 | `00 c8 01 40 00 05` | 320x200, 5 planes | 40006 |
| EXTER 1000 | `00 64 01 40 00 05` | 320x100, 5 planes | 20006 |
| URBAN 1300 | `00 7d 00 a7 00 05` | 167x125, 5 planes | 13756 |

The MCGA entry-4 planar path and the even-stride padding byte agree with this.
`soe_video::decode_planar` implements it with tests. Rendered PNGs are in
`out/img/palette/` (git-ignored).

Some records carry trailing bytes; the decoder accepts `payload.len() >=
6 + planes*height*stride` and ignores the remainder. This recovers
`INTF#501`, the 320x200 ornate map frame (centre transparent) seen in the
original screenshots.

## Flag-0x01 records (20)

ANI 705, 706, 710; EXTER 1037, 1058, 1243, 1244, 1245; EXTER2 1036;
INTER 1118, 1123; INTF 201, 500, 501, 652, 806; LOGO 10001, 10057;
TITLE 120, 121. Their RLE token layout is documented in `COMPRESSION.md`.

## Palettes (verified room/world/UI tables)

Three 32-entry tables of 4-bit RGB components live in DGROUP:

- rooms: DS `0x2099` (file offset `0x280D9`), also duplicated at DS `0x2039`
- DS `0x20F9`
- DS `0x2159`

Code sets a palette pointer at DS `0x8E5D` to `0x2039`, `0x20F9` or `0x2159`
(`2d65:0001`, `2b79:0003/0397/0aca`); the room renderer copies from `0x2099`.
Components are expanded with `v * 17` (4-bit -> 8-bit), not shifted as 6-bit.
ENGLAND/EXTER/URBAN were re-rendered with the `0x2099` palette. TITLE.RES's
dedicated palette is **not located**; prior notes also leave it open.

## `soe img dump`

Not added yet: room/world/UI palettes are verified, but TITLE/LOGO palette
association and the ANI frame-bank wrapper (0x05 prefix) are still unresolved.
Adding it now would emit wrong-colour TITLE/ANI assets.


## Oracle and image dump (follow-up 10)

- Unicorn real-mode oracle: `tests/oracle/excal_oracle.py` loads the unpacked
  payload at 0x1000:0000. **LZSS 523/523 records match byte-for-byte.**
  Flag-0x01 RLE layout matches; 15 records differ only because the original
  final run reads past the record tail into adjacent memory (oracle overrun),
  not because the token layout is wrong.
- `soe img dump <file.res> <outdir> [--palette room|world|ui]` now exists.
  It classifies images by exact planar size and writes indexed PNGs plus JSON
  sidecars and `palettes.json`.
- Whole-corpus dump: **422 images** from 841 records. Non-image inventory:
  **187 text**, **26 SMUS**, **206 unknown** (includes SCENES scene data,
  ANI frame banks and unclassified tables).
- Default palette is the room table (marked `default:false` when the EXE table
  is found). TITLE/LOGO dedicated palettes remain **unverified**; their sidecars
  therefore use the default room palette and should not be treated as final.

## Palette selection rule (verified)

Code writes DS `0x8E5D` with `0x2039`, `0x20F9` or `0x2159`; the room renderer
copies DS `0x2099`. `0x2039` and `0x2099` are identical 96-byte tables.
`0x20F9` and `0x2159` are the other two 32-entry 4-bit tables. Components are
expanded with `v * 17`. Callers: `2d65:0001`, `2b79:0003`, `2b79:0397`,
`2b79:0aca` write `0x8E5D`; the renderer copies `0x2099`.

## Scene/ANI status

- Prior `scenes.md`/`ani.md` claims were read. SCENES.RES record schema (room
  background id, object list, actor/animation ids, hotspots/walkable areas) is
  **UNVERIFIED** against the EXE. ANI frame-bank wrapper after flag-0x01 decode
  is **UNVERIFIED**; the RLE token layer is verified, but frame count/offsets/
  anchors/transparency are not.
- Consequently ANI frames are not emitted as individual PNGs yet.


## Palette triage (follow-up 10b)

Rendered every ENGLAND record except 510 with the three candidate palettes in
`out/triage/` (plus checkerboard transparency versions). Findings:

- The small ENGLAND images retain **brown/red coastline colours under the room
  palette `0x2099`**; the map `0x20F9` and UI `0x2159` tables render them as
  near-greyscale. Geometry was already correct.
- `ENGLAND#511` (38x51) is about **75% palette index 0**, so it is an overlay/
  silhouette, not a standalone picture. Treat index 0 as transparent for these
  small map overlays. Other records range from 24% to 59% index 0.
- `ENGLAND#510` is the full 320x200 map and uses the **map palette `0x20F9`**;
  the map path copies `0x20F9` into the active buffer at DS `0x9BA0` and sets
  `DS:8E5D = 0x9BA0` (`2b79:0003`, map region).
- Room/UI path: room images copy `0x2099`; interface images use `0x2159`
  (`2b79:0397/0aca`, party/journal/combat). The active palette is always copied
  to DS `0x9BA0`, and `DS:8E5D` points at that buffer.

Verified default per file/group now used by `soe img dump`:

| group | palette |
|---|---|
| ENGLAND#510 (full map) | map `0x20F9` |
| ENGLAND#511..526 (overlays) | room `0x2099`, index 0 transparent |
| EXTER/EXTER2/INTER/URBAN rooms | room `0x2099` |
| INTF interface | UI `0x2159` |
| TITLE/LOGO | default room, marked `default:true` (still unverified) |

Still failing after palette/transparency: TITLE and LOGO images (dedicated
palette not found), and ANI frames (frame-bank wrapper not decoded, so no ANI
PNGs are emitted). No geometry/layout variant was needed for ENGLAND.


## Type IDs (follow-up 2b)

Directory type IDs are 4 bytes stored byte-reversed. Distinct IDs found:

| stored | normal | records | interpretation |
|---|---|---:|---|
| GAMI | IMAG | 281 | images |
| RTSC | CSTR | 195 | text |
| LMRF | FRML | 41 | ANI frame banks |
| NECS | SCEN | 138 | scene data |
| SUMS | SMUS | 25 | music |
| PAMM | MMAP | 127 | interface/map (INTF, together with IMAG) |

TITLE has **IMAG + SMUS** sections (31 images + 3 non-image records). INTF has
**MMAP + IMAG** (119 images + 8 non-image records). LOGO is **IMAG** (6 images +
1 non-image record).

There is **no palette type**. No 32-entry palette record was found in TITLE or
LOGO; the dedicated title/logo palette remains unlocated (still `default:true`
in sidecars).

`soe img dump` now tags every manifest entry with its section type ID(s):
non-image inventory is CSTR text/unknown, FRML unknown, SCEN unknown, SMUS
music, and IMAG/MMAP unknown rows.


## TITLE/LOGO palette candidate (START.EXE)

START_UNPACKED.EXE contains two 32-entry 4-bit tables in its DGROUP at DS
`0x0284` and `0x02B6`. Rendering TITLE#122 and LOGO#10005 with DS `0x0284`
produces coherent title/logo colours (green/yellow/black title; yellow/black
logo) instead of the room palette's blue/orange. No direct code reference to
`0x0284` was found in the START export, so this is a **strong hypothesis,
UNVERIFIED**. The tool still marks TITLE/LOGO as default until the reference or
a runtime capture confirms it.
