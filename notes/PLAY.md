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
| M | toggle the regional map (arrows select, Enter travels, M/Esc closes) |
| H | open the story/hint chronicle (arrows/click page, H closes) |
| A | room action menu (arrows select, Enter, A closes) |
| Click a room character | select that character and open their action menu |
| Enter in dialogue | close, or apply the highlighted choice |
| T | travel map (arrows select, Enter/click travels, T/Esc closes) |
| S | save/load browser (Enter load, F5 quick-save, S close) |
| P | pause / close the pause panel |
| F2 | mute/unmute music |
| F5 | quick-save to `QUICK.SAV` |
| F9 | quick-load from `QUICK.SAV` |
| Esc | quit |

Startup flags:

```text
--scene N   start at scene id N
--map       start in map view
--status    start in status view
--story     start in the story/hint chronicle
--travel    start in the travel map
--saves     start in the save/load browser
--actions   start in the room action menu
--items     start in the item-selection panel
--paused    start with the pause panel open
--battle    open the demo army battle from the current save
--save FILE load a specific .sav instead of EPISODE1.DAT
```

Animations advance at the BIOS tick rate (18.2 Hz) and every scene is clipped
to the 320x200 logical frame. Presentation is a nearest-neighbour scale to the
window (default 640x400).

## Scope

The interaction loop is playable: room character selection opens the action
menu, Talk resolves the event table to a `HINT.RES` line, Give/Drop/Pickup/Use
move live object instances, and join/plea dialogue records offer choices.
Combat, per-scene event visibility and driver-exact audio remain open. This is
not yet a complete game.
