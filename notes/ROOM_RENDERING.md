# Room composition

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
