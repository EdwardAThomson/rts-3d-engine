# The renderer

Draft, 8 Oct 2026. Roadmap step 5. `crates/render3d` draws a game with wgpu on `rts-platform`, the platform layer
shared with the Classic engine (a second crate in the `rts-core` repository, pinned by commit; Ed's choice,
8 Oct 2026). The camera, terrain mesh and picking it uses are in `crates/view3d` (docs/view.md).

## Rules

1. **An observer.** It reads the world (and later `events`) and never changes it. The viewer gives orders only
   through `World::command`, like any player.
2. **What to draw is worked out without a GPU.** `Shapes` turns the world into boxes in view space, so the rules
   for what appears where are tested with no adapter. A mobile unit is a box the size of its disc and as tall as its
   type; a structure covers exactly the cells the simulation blocks; a frame is pale and grows with the work done
   on it; projectiles, wrecks and resource spots have boxes of their own. Players' colours are generic, by owner
   number.
3. **Smooth at any frame rate.** The simulation keeps its fixed tick. Mobile units and projectiles slide from where
   they were at the tick before to where they are now, so the picture runs one tick behind the state.
4. **One pass.** The terrain mesh (at the level of detail the camera asks for, kept once made) and then every box
   as an instance of one cube, over a depth buffer, lit by one low sun from the west-north-west. Faint lines along
   the cell edges are draped over the ground, so its shape reads from any angle even where hills are gentle; they
   fade out when cells get too small on screen. Undersides of the terrain are culled; boxes draw both ways round.
5. **Tested offscreen.** `Renderer::draw_to_image` draws with no window on any adapter, a software one included.
   The tests check the map against the sky, each player's colour where their units are, that a ridge hides a unit
   behind it, and leave frames in `target/` to look at.
6. **Playing goes through `control`, with no GPU.** Selecting and ordering are worked out from the world, the
   camera and the shapes on screen, so they are tested without an adapter. The cursor's ray picks the nearest unit
   or frame box it enters, unless the ground (by `view3d::pick`, on the simulation's own heights) is nearer; a box
   selects every unit of yours whose middle projects inside it. Orders go in as ordinary commands in sub-cell
   units, so nothing new reaches replays or the state hash. A selected unit gets a pale plate under it, and the
   drag box is flat rectangles drawn over the scene.

## The viewer

```bash
cargo run --release -p render3d --bin play3d -- --seed 1 --players 2
```

You play the first side (blue) of the generic skirmish (`ai3d::skirmish`) against computer players. Left-click
a unit of yours to select it or drag a box round several, with shift to add to the selection; right-click an enemy
to attack it or the ground to move there, with Ctrl held to attack-move. A computer helper (an ordinary `ai3d`
player) runs your base and factories and sends waves with the fighters you leave to it; once you give a unit an
order it is yours alone, and the helper's orders for it are dropped. That lets you play before there is a HUD for
building, an idea we take from Supreme Commander's and Total Annihilation's automation of the chores. `--watch 1`
(`?watch=1` in the browser) leaves every side to the computer. The wheel zooms at the cursor, arrow keys or WASD
pan, Q and E turn, space pauses, + and - change the speed (1x to 32x), Home shows the whole map and Escape quits.
The game runs at 30 ticks a second of game time; that rate is the viewer's choice, since the simulation has no
clock.

The same viewer runs in the browser, drawing with WebGPU, or WebGL2 where the browser has no WebGPU, with the
options in the page address (`?seed=3&players=4&speed=8`):

```bash
cargo build --release --target wasm32-unknown-unknown -p render3d --bin play3d
wasm-bindgen --target web --no-typescript --out-dir web/play3d/pkg target/wasm32-unknown-unknown/release/play3d.wasm
python3 -m http.server 8000      # then open http://localhost:8000/web/play3d/
```

`wasm-bindgen` is the command-line tool of the same version as the library in `Cargo.lock`. CI runs
`web/play3d/check.mjs` in headless Chromium on both WebGPU and WebGL2: the game ticks, the frame shows the map,
Space pauses, a drag selects, a right-click opens no browser menu and the wheel zooms.

## Later

- Building and production from a HUD, so the helper can be switched off; double-click to select every unit of a
  kind on screen; control groups.
- Models from the art studio's glTF exports in place of boxes, with icons when zoomed far out.
- A HUD with the sprite batcher and font from `rts-platform`, and sound from its mixer.
- Effects from `events`: muzzle flashes, impacts, wrecks burning.
- Units tilted to the slope they stand on; today a level box sinks into a hillside.
