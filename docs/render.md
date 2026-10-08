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

## The viewer

```bash
cargo run --release -p render3d --bin play3d -- --seed 1 --players 2
```

Computer players fight the generic skirmish (`ai3d::skirmish`). The wheel zooms at the cursor, arrow keys or WASD
pan, Q and E turn, space pauses, + and - change the speed (1x to 32x), Home shows the whole map and Escape quits.
The game runs at 30 ticks a second of game time; that rate is the viewer's choice, since the simulation has no
clock.

## Later

- Playing, not only watching: selecting units with the mouse and giving orders through `view3d::pick`.
- Models from the art studio's glTF exports in place of boxes, with icons when zoomed far out.
- A HUD with the sprite batcher and font from `rts-platform`, and sound from its mixer.
- Effects from `events`: muzzle flashes, impacts, wrecks burning.
- The browser build, as the Classic engine's player does.
- Units tilted to the slope they stand on; today a level box sinks into a hillside.
