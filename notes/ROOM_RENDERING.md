# Room composition

Status: **ported** (`soe_engine::room_builder`, verified against the original:
`tests/oracle/room_vectors.py` records 415 builds of all 138 `SCENES.RES`
records under three DGROUP set-ups plus a real duel, and
`room_builder::tests::matches_recorded_builds` replays them: widgets byte for
byte, every blit, the return value, the globals and the palette).

`FUN_2b79_0003` is what decides what a room shows; see `SYMBOLS.md` for the
details. The presenter (`room::draw_background`, `room::draw_widgets`) draws
the fill colour, the stamped stills and then the widgets back to front; images
stay handles (`room::SceneImages`) that map to resource ids, cut-outs being a
rect of a still (`room::Cut`).

Recording: `python tests/oracle/room_vectors.py NAME hall 0 --direct LOC:MODE...
[ADDR=BYTE...]` calls `FUN_2b79_0ea5`/`FUN_2b79_0003(1)` at the start of a
scene frame, saving a screenshot `tests/vectors/emu/room_NAME_LOC_MODE.png` after
each build; the pokes reach the scene-mode, army and lying-object paths.

Known approximations outside the builder:
- the session builds rooms over the EXE's initial DGROUP (scene mode 0, no
  campaign battle), and does not run the scenery scripts: animations cycle
  their bank's frames;
- random placement of lying objects uses a fixed sequence (the original's is
  timer-seeded);
- `FUN_1772_0578` (battle-panel save-under) and the screen hand-offs
  `FUN_1316_1143/1111`, `FUN_17f3_003a` are not ported.

`room::compose_scene` (draw the record's objects in order) remains only for
`soe room compose` and headless sessions without DGROUP; it shows objects the
original leaves out.

---

Earlier notes:


Status: **renders coherently** for `SCENES.RES` scene 1.

`soe_engine::room::compose_scene` walks `Scene::objects` in file order and blits
each resolved still image with palette index 0 transparent. Coordinates are
clipped; `x` is signed, `y` is the top edge. `flags & 1` mirrors the image
horizontally (confirmed in `FUN_2b79_0003` and `FUN_14b0_0bbf`); other flag
bits are ignored. The background byte is ignored
(the prior notes say the renderer does not need it).

`soe room compose <SCENES.RES> <scene-id> <RES32-dir> <out.png>` loads every
`RES32/*.RES` archive, resolves object ids across archives in name order,
decodes 5-plane images, and uses the room palette (`EXCAL DGROUP DS 0x2099`,
6-bit entries expanded with `v * 17`).

Scene 1: 16 objects, 12 static images, 1 animation. The result shows the
complete village scene (sky, hills, buildings, well, knights). Animation objects
are rendered from frame 0; all FRML frames decode, and timing/frame selection is
the remaining step.

Cross-archive resolution is required: scene 1 uses `EXTER.RES#1039/#1041`,
`URBAN.RES#1403/#1407`, and `ANI.RES#908`.
