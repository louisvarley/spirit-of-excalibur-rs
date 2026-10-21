# Main loop and scene renderer

Status: **main-loop state machine ported** as `soe-engine::runtime::Runtime`.
The scene renderer is still the static room compositor; animation frames,
palette cycling and driver callback details remain open.

## Runtime state machine (`FUN_2d65_2cd9`)

`Runtime` owns the loaded `Session`, the current `Screen`, the original loop
word and all menu selections. `soe-app` no longer keeps mode booleans; it maps
platform input to `runtime::Key`, calls `Runtime::handle_key`/`handle_click`,
and applies the returned `RuntimeEvent`s.

The original word at `DS:01EC` is represented by `LoopState`:

| word | `LoopState` | behaviour |
|---:|---|---|
| `-5` | `ExitToMenu` | tear down the view, then fall through to the menu |
| `-4` | `GameLost` | show `GAMELOST`; confirm requests quit |
| `-3` | `ScreenChange` | finish a deferred travel/map screen change |
| `-2` | `Menu` | menu/idle frame; no room animation |
| `-1` | `Playing` | normal play frame |
| `>= 0` | `Playing` | original play frames (intro/room/menu modes) |

`Runtime::state_code()` reports the canonical code and
`LoopState::from_code` mirrors the original `>= -1` test. The code writers in
the original were used to place the transitions: room entry sets `-1`,
intro/room modes use `0`/`1`, travel/map interactions use `-3`, the
game-lost path uses `-4`, and the post-combat return path uses `-5`.

Every screen is a `Screen` variant: `Menu`, `Room`, `Map`, `Status`, `Story`,
`Travel`, `Save`, `Pause`, `Action`, `Item`, `Spell`, `Shop`, `Target`,
`Dialogue`, `GameLost`, `Quit`.

### Transitions

- `Room` -> `Map` with `M`; `Map` -> `Room` with `M`, `Esc` or `Enter`.
- `Room` -> `Status` with `I`; up/down selects a knight and `Enter` opens
  their live inventory. `I`/`Esc` closes. The panel decodes stats, purse, gear
  and prepared spells from the save state.
- `Room` -> `Story` with `H`; arrows/space/click page, `H`/`Esc` closes.
- `Room` -> `Travel` with `T`; up/down selects, `Enter` enters the location's
  scene through `-3`, then returns to `Room` when the fade completes.
- `Room` -> `Save` with `S`; up/down selects, `Enter` loads, `F5` quick-saves,
  `S`/`Esc` closes. Loading replaces the session and returns to `Room`.
- `Room` -> `Pause` with `P`; any key resumes the room.
- `Room` -> `Action` with `A`/`Enter`; Talk/Trade/Give go to `Target`,
  Pickup/Drop/Use go to `Item`, Magic opens `Spell`. Target Talk opens
  `Dialogue` (or the chronicle when no event exists), Target Trade opens
  `Shop`, Target Give opens `Item`. Item/spell/trade rows act on the live
  save state; cancel returns to `Room`.
- `Room` left/right arrows or clicks change scene; `Space` advances one
  animation frame. Animation ticks only run in `Room` while `-1`.
- `GameLost` confirm requests quit. `return_to_menu()` enters `-5`; the next
  `tick` resolves it to `-2`/`Menu`, and confirm starts `-1`/`Room`.

`Runtime::tick` advances the fade and animations and resolves `-5` and `-3`.
The presenter reads `Runtime::fade()`; all coordinates and clicks stay in
logical 320x200 space.

## Verification

The live DOSBox-X save states were inspected only as a transition oracle, not
added as fixtures or tests. They show normal play/game-over at the expected
loop words, which matches the `>= 0` / `-1` play rule and the terminal `-4`
path. State-transition tests use the headless runtime, plus one integration
test that drives a real episode through map, status, pause, travel and save.

## Startup

`entry` (`1000:0000`) runs the Turbo C startup and calls `FUN_2d65_2cd9`. The
latter:

1. Installs handlers / driver callbacks.
2. Allocates buffers with `FUN_14b0_0122(200)` and `(300)`.
3. Sets the palette selector `DS:8E5D = 0x1FD9`, active buffer `DS:9BA0`.
4. Registers driver entry points at `DS:8A43`, `DS:8AB6`, `DS:8E19`...
5. Loads a save with `FUN_1a51_059d(DS:798E, DS:7990, DS:8E3D)`.
6. Runs the infinite state loop on `DS:01EC` described above.

The VGA mode is set through the driver callback at `DS:8E65` (MCGA or EGA/Tandy
variant).

## Scene renderer (`FUN_2b79_0003`)

- Object array pointer: `DS:9D14`; count: `DS:9D18`.
- Per object: id = `u32 & 0x7FF`; x = `(next_word & 0x1FF) - 190`;
  y = `word & 0xFF`; flip = `flags & 1`.
- Animation ids: `700..=799` and `900..=999`; every other id uses the static
  image path.
- Static: `FUN_14b0_0122(id)` gives header `{... width +8, height +10}`.
- Animation: `FUN_14b0_0251(id, 0)` looks up `FRML` by id, advances a frame
  counter, fixed height 20, width from frame metadata.
- Blit flags are `0x46 | flip`; `FUN_14b0_0bbf` folds bit0 into the sprite
  control word (`control ^= (flags & 1) << 7`), confirming horizontal mirroring.
- Id `1000` sets `local_a = -0x18` (special boundary/culling case).
- The renderer also cross-fades cached frames when `DS:9D19 != 0`.

## Room compositor mapping

`soe-engine::room` currently implements the static path: signed x, top-edge y,
palette index 0 transparent, file-order drawing, `flags & 1` horizontal flip.
Animation frames and the special 1000 case are not yet ported.
