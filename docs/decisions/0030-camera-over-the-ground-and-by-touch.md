# ADR 0030: The camera stays over the ground, and every camera move can be made by touch alone

- Status: accepted and built, 2026-11-28 (step 6 of the [3D proposal](../proposals/3d-architecture.md), piece 6.2)
- Decided by: the owner approved River Town's spec ([docs/games/river-town.md](../games/river-town.md)),
  whose "Controls" ask for this: drag pans, pinch or wheel zooms, two buttons turn the view a quarter turn,
  one switches the view, every camera move by touch alone, the camera's center over the valley. How each is
  done was chosen by measurement; the tables are below. No format key was added and no budget limit was
  raised.
- Builds on: [ADR 0020](0020-render-list-camera-and-cpu-reference-renderer.md) (the camera is session state),
  [ADR 0022](0022-native-player-winit-and-rwgpu-play.md) and [ADR 0024](0024-web-player-on-webgpu.md) (the
  players and their bindings).

## Context

River Town's valley (about 40 × 28 cells) is larger than the screen: the game can only be played with the
camera, and it must be playable on a tablet with no keyboard and no mouse. Before this piece a pan could take
the camera's center off the ground, where the player sees nothing and cannot get back except with Home, and a
touch screen had a tap, a swipe and a pinch: no pan, no turn, no change of view, no way back.

## Decision

1. **The camera's center stays over the ground: a rule of the engine.** `Session::camera_bound`
   (`rwgpuengine_render::camera::Bound`) is fixed when a session starts, and `Session::move_camera` keeps the
   center inside it after every move (`Camera::apply_within`). Every move goes through there, so a playtest's
   `[[input]] camera`, a gesture in the window or on the page, and Home all keep it; a playtest replays it
   exactly, natively and in wasm, because the clamp is two exact `min`/`max` per axis.
   - With a ground: its cells edge to edge, from (−0.5, −0.5) to (size_x − 0.5, size_y − 0.5).
   - Without a ground: the cells of the world entities that take up space (`Entity::takes_space`) when the
     session starts. A scene with neither: the starting point alone.
   - Either way the bound holds the point the scene's camera starts at, so a scene whose `[camera] center` is
     outside its ground starts there unchanged, and Home can always go back there.
   - `rwgpu run`, `rwgpu inspect` and the web player report it with the camera: `"bound": [min_x, min_y,
     max_x, max_y]`.
   - No format key: the bound follows from the ground. The zoom keeps its range (0.125–16).
2. **Every camera move by touch alone, in both players**, from one table in
   `players/rwgpuengine_player/src/input.rs`:

   | touch | does |
   |---|---|
   | one finger, still (moved less than 12 frame pixels) | a tap, as before |
   | one finger, dragged | in a game with no mover: a pan, the ground under the finger following it. With a mover: a swipe, as before |
   | two fingers | the ground under them follows them: moved together they pan, apart or together they zoom about their midpoint, and once they have twisted 20° they turn the view about it |
   | letting go of a twist | the view settles on a quarter turn: the fingers' twist in whole quarter turns, at least one the way they twisted |
   | two fingers, still | switch between iso and perspective (V) |
   | three fingers, still | back to the scene's camera (Home) |

   - The left mouse button follows the same rule as one finger: in a game with no mover a left-drag pans.
     Every other mouse and key binding of ADR 0022 is unchanged.
   - **The web page has buttons outside the frame**, beside the direction pad: ⟲ and ⟳ (Q and E, a quarter
     turn each way), View (V) and Home. The native window has no controls of its own: drawing them would be a
     visual feature in a player. There the gestures are the way, and they are the same code.
   - **The page stops the browser's own gestures on the canvas**: `touch-action: none` (as before), no
     selection or callout, `overscroll-behavior: none` on the page, and `preventDefault` on `touchstart`,
     `touchmove`, `dblclick` and Safari's `gesturestart` and `gesturechange`.
   - The window takes winit's `Touch` events. A system that also makes mouse events out of touches (Windows)
     has those ignored while a finger is down.
   - A grab is one `CameraMove` (turn, zoom, then the pan that takes the ground point under the old midpoint to
     the new one, solved through the projection, `play::grab_move`), and a settle is one move by the plain
     difference to the quarter turn. So whatever a person does is a list of moves a playtest can make.

## Measurements

### One finger: a pan or a swipe

Actions drive only movers (`world/movement.rs`); anywhere else a swipe writes an `input` event and moves
nothing. The examples, and what one finger's drag could do in each:

| game | a mover | drag always pans | drag always swipes, two fingers pan | **by the game: pans with no mover** |
|---|---|---|---|---|
| `tiny_town` | no | pans | nothing; panning needs two fingers | **pans** |
| River Town (spec) | no | pans ("drag pans") | nothing | **pans** |
| `coin_vault`, `iso_garden` | `player` | the player cannot be moved by touch in the window (the page has a pad) | moves the player | **moves the player; two fingers pan** |

`play::drag_pans` decides it when a session starts: no entity and no prefab the scene spawns has a `mover`.

### Two fingers: telling a pan, a pinch and a twist apart

Measured in `players/rwgpuengine_player/src/gestures.rs`: 4,000 seeded two-finger gestures per level of
drift, a quarter each of pans (60–300 pixels), pinches (0.5–0.8 or 1.25–2 times, about the midpoint or with
one finger still), twists meant as a quarter turn (20–90°) and two-finger taps. Each is played through `Input`
as a touch screen sends it (one finger's move, then the other's, 24 times, each point off by up to 1.5 frame
pixels) and is made with some of the others, as hands do: drift of the angle ±5 / 10 / 20°, of the spread
±5 / 10 / 20 %, of the midpoint 10 / 20 / 40 pixels (low / medium / high). **This is a model of hands, not
recorded hands**: no person's touches were recorded for it. What it settles is how each choice trades a turn
nobody meant against a turn that does not happen, under the same drift.

Turning starts after a twist of (rates in %; zoom at once):

| twist | pans and pinches that turned, medium drift | the same, high drift | twists of 20–45° that turned | twists of 45–90° |
|---|---|---|---|---|
| 0° (at once) | 99.8 | 100.0 | 100.0 | 100.0 |
| 10° | 2.7 | 49.9 | 100.0 | 100.0 |
| 15° | 0.0 | 24.9 | 100.0 | 100.0 |
| **20°** | **0.0** | **1.6** | **98.3** | **100.0** |
| 25° | 0.0 | 0.0 | 81.2 | 100.0 |
| 30° | 0.0 | 0.0 | 62.7 | 100.0 |

Zooming starts (twist 20°, medium drift):

| zoom | pinches more than 5 % from the fingers' spread | pans whose zoom changed by more than 5 % | the view jumps |
|---|---|---|---|
| **at once** | **1.4** | 50.3 | no |
| after a 10 % change | 40.4 | 12.2 | no |
| after a 10 % change, catching up | 1.4 | 28.2 | by 10 % when it starts |

Turning and zooming as one or the other (whichever starts first): with zoom at once, turning never starts
(0–2.6 % of twists turn); with zoom after 10 %, it saves 1.3 points of unmeant turns at high drift and loses
24 points of small twists. Two-finger taps switched the view in 100 % of every row.

- **Twist 20°** is the smallest threshold with no unmeant quarter turn at medium drift and 1.6 % at high,
  and it still turns 98 % of the small twists; 25° and 30° lose a fifth and a third of them.
- **Zoom at once**: the zoom is the fingers' spread, about their midpoint, with no jump. The cost is in the
  table: a pan zooms as much as the fingers' spread drifts (half the pans at medium drift end more than 5 %
  zoomed). Starting after 10 % without catching up leaves 40 % of pinches short; catching up jumps.
- Both run together, not one or the other.
- `cargo test -p rwgpuengine_player gesture_table -- --ignored --nocapture` prints every row (21 tunings,
  each at the three drifts); `the_chosen_tuning_keeps_its_measured_rates` holds the chosen tuning to its row
  in every run.

### The rest, by measurement

- **Settling is exact.** From 5,000 random turns, quarter turns and twists, `settle_move` lands on the quarter
  turn bit for bit. A move nudged by last bits to land exactly on any target was measured too, against the
  plain difference, over 4,000 random cameras: it missed as often as the difference on the turn (431) and the
  center (1,295), and 412 against 470 on the zoom, because a move longer than its target is large has no f32
  that lands there. It was not kept; quarter turns, the case that matters, are exact either way.
- **A grab keeps the ground under the fingers** to 0.05 frame pixel, under five cameras (iso, turned, zoomed,
  perspective), panning, zooming and turning at once; a clockwise twist turns the ground clockwise.
- **What the bound leaves on screen** (Tiny Town, the part of the frame that is not the HUD): with the center
  at the ground's corner, 22.5 % shows the scene in the iso view and 28.3 % in perspective; at an edge's
  middle, 26.8 % and 19.9 %. A bound at the cells' centers instead (half a cell in) shows 25.0–33.3 % at the
  corner; it was not taken, because the center would then not reach the cells along the edge, which a player
  pans to in order to build there.

## Evidence

- **No hash changed.** All ten example playtests end in the state and image hashes they had (compared with
  `rwgpu shot` built from `river-town` before this piece), and no reference image changed. No camera of
  `cargo xtask gpu`'s scenes goes past a bound: the farthest, a pan of 2 cells from the middle of a ground of
  4, ends on it.
- **A new playtest, `examples/tiny_town/tests/past_the_edge.toml`,** pans 12 cells east (the center stops at
  x = 9.5), then turned, zoomed and 30 cells past the south-west corner, then in perspective past the
  north-east one, and taps after each: a tree at (8, 2), a house at (2, 4), a well at (6, 1), 6 coins left,
  one reference image (`refs/past_the_edge.png`, looked at: the island's corner at the middle of the frame).
  On the engine before this change the same file fails: nothing is built and the coins stay at 12.
- **The browser check touches the page** (`check.mjs --touch`, run by `cargo xtask gpu`): Tiny Town on a
  700 × 700 phone-sized page with touch emulation, the touches sent through DevTools
  (`Input.dispatchTouchEvent`), the page paused so that each finger down or up takes one step: one finger's
  pan, a tap that builds, a pinch to 1.5 times, the Tree button, a tap, a twist of 40°, a tap, a tap of two
  fingers, a pan far past the edge, a tap of three fingers, a tap. Nine checks: the pan moved the center;
  the zoom is 1.5 within 3 %; the turn is exactly 90; the view is perspective; the center stopped on the
  bound; Home is the scene's camera (center and zoom within 1e-5); four things were built; the page neither
  scrolled nor zoomed and its canvas has `touch-action: none`; the same drag on the line under the canvas
  does scroll the page, so a scroll would be seen. Then xtask writes what the page recorded (61 to 65 camera
  moves, as Chrome merges some touch moves, and 5 taps) as a playtest and `rwgpu run` replays it: the same
  state hash and the same camera, bit for bit.
  Run against a copy of the page with `touch-action` and the `preventDefault`s taken out, 4 of the 9 checks
  fail (the browser cancels the pinch at 1.21, the twist and the long pan; `touch-action` reads `auto`).
- **Unit tests** of every binding: taps and swipes with a mover, pans without one, the mouse's bindings, one
  finger, two fingers panning and zooming, the twist and its settle (both ways, back below 20°, two quarter
  turns at 140°, a cancelled finger), two- and three-finger taps in any order of lifting, the finger left
  after two, the owned arctangent; the grab, the settle and `drag_pans` on the examples; the bound in the
  runtime (gesture moves, a playtest that taps after a pan past the edge, a scene with no ground, a camera
  that starts outside its ground).
- `rwgpu help play`, `rwgpu help serve`, the page's help and docs/reference/cli.md say the new bindings; tests
  keep the page's help and `rwgpu help play` naming every key and every gesture.

Sizes, built on an Apple M5 Pro from `river-town` before this piece and with it:

| budget | before | now | limit |
|---|---|---|---|
| web module (`rwgpuengine_web`): the bound, the camera's `bound`, the schema's words for it | 704,851 bytes | 706,544 | 800,000 |
| players: web download (the step's figure before: 825,103) | 825,107 bytes | 838,406 (325,449 gzipped) | 1,500,000 |
| of which the player's wasm / its glue / `player.js` / `index.html` | 726,138 / 79,045 / 15,086 / 4,838 | 734,243 / 80,706 / 17,105 / 6,352 | — |
| `rwgpu` | 2,334,496 bytes | 2,334,576 | 3,000,000 |

Every other budget row is unchanged within its usual spread (`cargo xtask ci`).

## Consequences and limits

- **Not tested on a real phone or tablet, and not on a real touch screen at all.** This Mac has none; the
  browser check uses Chrome's touch emulation, and the native window's `Touch` events are not exercised by
  any test (winit gives none on macOS). Windows' mouse events made from touches are ignored while a finger is
  down, which is untested too.
- In a game with a mover, one finger swipes, so panning needs two fingers there.
- A twist always settles on a quarter turn: by touch there is no free turn (right-drag still turns freely).
  The settle is one move, not an animation.
- The bound is fixed at the start: a scene with no ground does not grow it as things move or are built.
- The zoom range is unchanged, so a player can still zoom out until the valley is small; the center is over
  it.
- The tap slop is 12 frame pixels, not a size in the screen's own points: on a frame shown at half size a
  finger has 6 screen pixels to stay still in. Unchanged from ADR 0024.
- Two fingers zoom about their midpoint; the wheel, a trackpad's pinch and the keys zoom about the frame's
  center, as before.
- The `play` entry of the CPU player (ADR 0008) still has no camera.

## Not done

Inertia after a pan, a double tap to zoom, a free turn by touch, an animated settle, a size for the tap slop in
screen points, the camera in the CPU player, an `[[expect]]` on the camera.

## Revisit when

A real tablet or phone is tested and the 20° or the zoom rule feels wrong in a person's hands; a game needs a
free turn by touch; a scene needs its bound to follow what it builds.
