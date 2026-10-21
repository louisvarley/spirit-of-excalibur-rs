# Packaging

`build-release.ps1` builds `soe-play.exe` and creates a clean distribution folder
containing only:

- `soe-play.exe`
- `README.md`
- `PLAY.md`
- `QUICKSTART.txt`

It refuses to package `.RES`, `.DVR`, `.SAV`, `.DAT`, image, archive or extra
`.exe` files.

The release deliberately does **not** include the original game, unpacked
executables, extracted assets, screenshots or save states. Users must supply
their own `game/` folder or set `SOE_GAME_DIR`.
