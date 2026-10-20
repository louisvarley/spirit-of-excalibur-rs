# Open questions

- **FRML**: fully decoded; see `notes/ANI.md`. Header is 2 bytes, descriptors
  are `u16 be offset, x, y, width, height`, and every frame is a raw planar
  body. No delta-frame codec exists.
- **MMAP records** (7 in INTF.RES): structure unknown.
- **TITLE/LOGO palette**: `TITLE#122` (320x200) renders coherently with the
  room palette (`DS 0x2099`): grey stone arch, blue window, green background.
  The START.EXE `DS 0x0284` candidate gives a false-colour yellow image and is
  no longer preferred for this frame. `LOGO#10005` is also plausible with the
  room palette (gold/blue spheres) but has no code reference. Status: visual
  evidence only, UNVERIFIED.
- **Font/text**: game text uses the atlas `TITLE.RES#200` plus EXCAL metrics
  `DS:1E8A..1E8C`; implemented. Still open: tenth-row clipping and whether any
  screen uses the MCGA driver's separate variable-width font.
- **SCEN renderer address**: schema verified; exact renderer routine and MMAP
  metadata still open.
- **Save loader oracle diff**: mapping is arithmetically resolved; full
  end-to-end diff needs DOS INT 21h emulation in the oracle.
