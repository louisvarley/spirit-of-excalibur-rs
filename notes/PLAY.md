# Playable build

`soe-play` is the first interactive build. It renders the original rooms with
animated objects and lets you walk through all 138 scene records.

## Run

```powershell
cargo run -p soe-app --release
```

Optional:

```powershell
cargo run -p soe-app --release -- --scene 1
```

Environment (defaults shown):

```text
SOE_GAME_DIR=game
SOE_EXE=ghidra/EXCAL_UNPACKED.EXE
```

The process needs the original game files; they remain git-ignored.

## Controls

| input | action |
|---|---|
| Left / right arrow | previous / next scene |
| Click left/right half | previous / next scene |
| Space | advance one animation frame |
| R | reset animations to frame 0 |
| I | toggle the Round Table/items status screen |
| M | toggle the world-map screen |
| Esc | quit |

Startup flags:

```text
--scene N   start at scene id N
--map       start in map view
--status    start in status view
```

Animations advance at the BIOS tick rate (18.2 Hz) and every scene is clipped
to the 320x200 logical frame. Presentation is a nearest-neighbour scale to the
window (default 960x600).

## Scope

This build does not yet implement combat, trade, dialogue choices, map travel,
inventory, audio, or the save/load menu. It is a playable room/animation viewer,
not a complete game.
