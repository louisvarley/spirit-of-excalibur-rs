# Decisions

- **Oracle**: Python + Unicorn real mode, payload at `1000:0000`. DOS/BIOS
  interrupts are not emulated yet; pure memory routines only.
- **RES schema**: use the verified 12-byte directory records
  (`id, 0xFFFF, u24 offset, flags, extra`) and longest-run section scan.
- **Flag-0x01 RLE**: `0x80` no-op, `0x00..0x7F` literal run, `0x81..0xFF`
  repeat. Original reads past record tail; Rust zero-pads, documented.
- **Image format**: big-endian `height,width,planes`; even byte stride;
  plane-interleaved rows, MSB first.
- **Palettes**: room `0x2099`, map `0x20F9`, UI `0x2159`, `v*17` expansion.
  ENGLAND#510 uses map; small ENGLAND overlays use room with index 0
  transparent. TITLE/LOGO use the room palette: `TITLE#122` is visually
  coherent, while the START.EXE `DS 0x0284` candidate is false-colour. The
  LOGO code reference is still unverified.
- **Save mapping**: main block is the first 9055 bytes of the file to DS
  `0x2C5C..0x4FBA`; the 22-byte block is the file tail to DS
  `0x4FBF..0x4FD4`. This is the arithmetic that matches character 0 at
  DS `0x2C8C`/name `0x2C96`.
- **No palette type** in RES directories. Type IDs seen: IMAG, CSTR, FRML,
  SCEN, SMUS, MMAP.

- **Windowed backend choice**: `winit` + `softbuffer`. Pure Rust, no SDL or
  GPU dependency, fast builds, and the CPU nearest-neighbour scaler accepts the
  indexed 320x200 frame directly. Keep `SoftwareVideo` as the reference
  renderer. `pixels` was rejected because current releases require a newer
  minimum Rust than the workspace.
- **Platform traits**: `Input`, `Timer`, `Sound`, `GameFiles` live in
  `soe-engine`; `Video` stays in `soe-video`. Game logic takes logical events
  and logical coordinates only.

- **FRML frame descriptors**: 2-byte header `format=5, frame_count`, then
  6-byte descriptors `u16 be offset, x, y, width, height`. Every frame is a
  raw planar body; there is no delta-frame codec.
- **SCEN trailer**: optional. Parse it when present; otherwise the record ends
  after the object list.
- **Room rendering**: object order is draw order, `x` is signed and clipped,
  `y` is the top edge, palette index 0 is transparent.
- **Game text font**: use the atlas `TITLE.RES#200` plus EXCAL metrics at
  `DS:1E8A..1E8C`, not MCGA entry 2's driver font. The atlas is 301x9; metrics
  define source x, width and overhangs per character.

