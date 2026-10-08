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
6. **Models stand in for boxes.** `model` reads the binary glTF files (`.glb`) the art studio's exporter writes
   (rts-engine, `art/studio/export_gltf.py --lod low`) with a small reader of our own and `png` for the baked
   textures. Each kind of the generic skirmish has one (`assets/skirmish/models/`, listed in `models.json` by kind
   name with the source studio model). Every model is drawn at one size per metre (`metres_per_cell`, the studio's
   10.67 metres to a tile), except that a building shrinks if it would not fit its footprint. A mobile unit faces the
   way it last moved (the simulation keeps no facing, so `Shapes` works it out from its moves), and a turret turns
   to its target. Team paint, baked grey, is multiplied by the owner's colour. A frame is the finished model, pale,
   rising from the ground as it is built. A wreck is its unit's model, burnt dark and squashed to a third of its
   height: a structure's over the footprint it still blocks, a mobile unit's as a low heap at its own angle, which
   ground units ride up over rather than through. Kinds with no model, shots and resource spots stay boxes. Textures
   get mipmaps so small far-off units don't shimmer. WebGL2 can't offset indices per draw, so each model's
   indices count from the start of its own buffer.
7. **Playing goes through `control`, with no GPU.** Selecting and ordering are worked out from the world, the
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
(`?watch=1` in the browser) leaves every side to the computer, and `--boxes 1` draws boxes in place of the models.
The wheel zooms at the cursor, arrow keys or WASD
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
- Icons when zoomed far out; the detailed models close in.
- Models for other settings, read from a setting pack instead of built into the program.
- A HUD with the sprite batcher and font from `rts-platform`, and sound from its mixer.
- Effects from `events`: muzzle flashes, impacts, wrecks burning.
- Units tilted to the slope they stand on; today a level box sinks into a hillside.
