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
   on it; wrecks and resource spots have boxes of their own. Shots are effects (rule 12). Players' colours are generic, by owner
   number.
3. **Smooth at any frame rate.** The simulation keeps its fixed tick. Mobile units slide from where they were at
   the tick before to where they are now, and shots fly along their own path (rule 12), so the picture runs one
   tick behind the state.
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
   ground units ride up over rather than through. Kinds with no model and resource spots stay boxes. Textures
   get mipmaps so small far-off units don't shimmer. WebGL2 can't offset indices per draw, so each model's
   indices count from the start of its own buffer.
7. **Playing goes through `control`, with no GPU.** Selecting and ordering are worked out from the world, the
   camera and the shapes on screen, so they are tested without an adapter. The cursor's ray picks the nearest unit
   or frame box it enters, unless the ground (by `view3d::pick`, on the simulation's own heights) is nearer; a box
   selects every unit of yours whose middle projects inside it. Orders go in as ordinary commands in sub-cell
   units, so nothing new reaches replays or the state hash. A selected unit gets a pale plate under it, and the
   drag box is flat rectangles drawn over the scene. A click also selects one of your structures, but a box takes
   only mobile units, and only mobile units take move and attack orders. Right-clicking a frame of yours sends the
   selected builders to help build it, and a wreck, to reclaim it. A double click selects every unit or structure
   of yours of that kind on screen. Right-clicking the ground with a factory selected sets its rally point
   (`Command::Rally`, docs/intent.md), drawn as a flag on a post while the factory is selected; units it finishes
   there are the person's, so the helper leaves them be. Ctrl and a number key keep the selection as a control
   group, the number selects the group again (shift adds it, an empty group changes nothing), and a second press
   within 0.4 seconds looks at the group's middle. Control groups and the double click are the viewer's own
   state; only rally points reach the simulation.
8. **The panel sits beside the scene.** `panel` is a sidebar down the right, 260 pixels wide, drawn with the
   sprite batch and pixel font from `rts-platform` first; the scene then fills the rest of the screen over it
   (`Renderer::set_area`: a viewport, and the colour is loaded rather than cleared). Top to bottom: a minimap (the
   ground shaded by height, spots, wrecks, every unit in its owner's colour, the camera's view as a dotted outline;
   click or drag on it to look there), a line per resource with its store's fill and what it gained or lost a
   second over the last two seconds (and a warning when spending outruns income), buttons for what the selection
   can make, the helper's switch and the clock. A structure's button starts placing it: a green or red plate shows
   where it would stand (`World::site_ok`), a click sends the nearest selected builder, shift places more, and a
   right-click or Escape stops. A unit's button queues one in the selected factory with the shortest queue, shows
   how many are queued and the first one's progress; a right-click empties those factories' queues. Factories and
   builders you give orders to are yours, as in rule 7. The names on it come from the setting (`Names`); the
   layout follows the classic 1990s sidebar. Like `control`, what a click does is worked out with no GPU.
9. **Effects come from `events`.** `effects` turns each tick's events into soft round blobs that face the camera
   (`Puff`), worked out with no GPU from the time since the event: a flash at the muzzle when a unit fires, a burst
   of fire and dust where a shot lands (bigger for a shell with splash, lifted so the ground doesn't cut its glow
   off in a line) with a few sparks thrown out and falling back, a blast where a unit is destroyed (bigger
   for a building), and smoke rising from every wreck, a vehicle's for 8 seconds and a building's for 30, with fire
   at a building's foot for the first third; smoke stops when the wreck is cleared. The renderer draws them last,
   tested against depth so hills and models hide them, without writing it; fire and flashes add light, smoke
   covers what is behind it, farthest first. A building going up also shakes the view for under a second, most when
   it is near the middle of the view (`shake`); only the picture moves, so clicks still land where they point.
10. **Sound comes from `events` too.** `sound` makes its clips in code (generic placeholders until a setting pack
   brings its own) and plays them through `rts-platform`'s mixer: a crack for a shot, a boom for a shell, a thud
   where it lands, a blast when a unit is destroyed and a bigger one for a building with a deep rumble that rolls on
   for a couple of seconds, and for your own side only, a
   clunk when a frame is placed and a chime when something is finished. Battle sounds are loudest near the middle
   of the view, fade over the ground it covers, are quieter off screen and when pulled right back, and pan to where
   on screen they happen. The sound card (the `sound` feature, on by default) opens at the start on the desktop and
   on the first click or key in the browser, which only allows sound after one; with no sound card the viewer is
   silent. M mutes.
11. **Menus first, as in any strategy game; the game-over panel in the panel's place.** `menu` draws over the whole
   screen with the same batch and font as the panel. The main menu comes first: the title and a centred column of
   Skirmish, Campaign and Load game, and Quit on the desktop (a browser tab has nothing to quit). Campaign is shown
   greyed out with "not yet" until the engine has campaigns, and Load game until there is a saved game. Skirmish (or Enter)
   opens the skirmish setup: a framed window in which the scene shows the map the options make, turning slowly
   (`Renderer::set_area_at` puts the scene's viewport anywhere on the screen), and buttons two by two for the map
   seed (1 to 999), the number of players (2 to 4), the helper and watching only, fog of war on a full-width row, each stepping on with a click and
   back with a right-click, then Back (or Escape) and Start (or Enter). When one side is left, the game-over panel
   takes the side panel's place beside the battlefield: who won, how long it took, and what each side built and
   lost (`menu::Tally`, counted from `Built` and `Destroyed` events), with Play again (the same options, or Enter)
   and Menu (back to the main menu). Clicks and keys are worked out with no GPU.
12. **Shots are drawn as what they are.** `shots` draws every projectile in flight, in the look the setting gives
   its unit kind (`assets/skirmish/shots.json`): a tracer is a bright streak, hottest at its head and fading back
   along its path; a shell is a glowing round with a faint smoke trail; a missile is a dark body with a flame at
   its tail and a thick smoke trail, with smoke billowing round the launcher. A kind the setting leaves out gets a
   shell if its weapon lobs and a tracer if not. In the skirmish tanks fire tracers and the artillery, whose model
   is a rocket launcher, fires missiles. A shot's flight is a closed form of the time since it was fired
   (`sim3d::weapon`), so `shots` rebuilds it from the `Fired` event and works it out again in floats at any
   fraction of a tick: the head is exactly where the simulation has the shot, and the trail lies along the path it
   really flew, left every tick or half tick and hanging in the air for a second after the shot lands. A `Puff`
   with a `stretch` is a streak: the shader lays a capsule along the stretch as the camera sees it, so end on it is
   a round blob. Like the other effects these only read; tracers and smoking rocket trails are the genre's usual
   look, and the code and numbers are ours.
13. **Fog of war is drawn for the side being played.** `fog::brightness` gives every cell a brightness (in sight
   255, fog 120, shroud 0) that the renderer uploads as a small texture, one texel a cell with linear filtering, so
   edges are soft; the ground, boxes and models are darkened by it in the shaders. Enemy units out of sight are
   left out (`Shapes::viewer`), enemy structures the side remembers are drawn where it last saw them, effects and
   sounds out of sight are dropped, and the minimap shades fog and blacks out shroud. Watching, the menus and the
   game-over panel show everything. The setup's Fog of war button (`--fog 0`, `?fog=0`) turns fog off for a game.
14. **Saved games replay; they are not snapshots.** `save` keeps how the skirmish was set up and everything the
   person did, each with its tick: their commands, the units they took from the helper and the helper's switch. The
   computer players' orders are left out: loading starts the same skirmish fresh and plays it forward, and the
   computer players (the helper too) think again exactly as they did, so their memory comes back without being
   saved. The state hash at the saved tick must match, so a save that doesn't reproduce its game is refused.
   Playing and loading run each tick through the same function (`save::tick`), so they can't drift apart. The
   file is text, one line per thing done; there is one save, kept by `store` in the user's settings folder on the
   desktop (`~/.config/rts-3d/`) and in the page's local storage in the browser. Escape opens the game menu in the
   panel's place, pausing the game: Resume, Save game, Load game and Menu; the main menu's Load game loads it too.
   Loading a long game takes as long as the simulation takes to play it at full speed. The shape is the Classic
   engine's saves (rts-engine `classic-render::save`), so both engines save the same way.

## The viewer

```bash
cargo run --release -p render3d --bin play3d -- --seed 1 --players 2
```

It opens on the main menu (rule 11); the command line's options fill it in, and `--menu 0` (`?menu=0`) skips it.
You play the first side (blue) of the generic skirmish (`ai3d::skirmish`) against computer players. Left-click
a unit of yours to select it or drag a box round several, with shift to add to the selection; right-click an enemy
to attack it or the ground to move there, with Ctrl held to attack-move. Double-click a unit to select all of its
kind on screen. With a factory selected, right-click the ground to set its rally point. Ctrl and a number keep the
selection as a control group; the number brings it back, and a second press looks at it. A computer helper (an ordinary `ai3d`
player) runs your base and factories and sends waves with the fighters you leave to it; once you give a unit an
order it is yours alone, and the helper's orders for it are dropped, an idea we take from Supreme Commander's and
Total Annihilation's automation of the chores. The panel on the right (rule 8) builds and produces; its switch
turns the helper off, so the whole side is yours, and `--helper 0` (`?helper=0`) starts with it off. `--watch 1`
(`?watch=1` in the browser) leaves every side to the computer, `--fog 0` (`?fog=0`) turns fog of war off, and `--boxes 1` draws boxes in place of the models.
The wheel zooms at the cursor, arrow keys or WASD
pan, Q and E turn, space pauses, + and - change the speed (1x to 32x), Home shows the whole map and Escape stops
placing a structure, or else opens the game menu to save, load or leave (rule 14). M mutes the sound.
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
Space pauses, a drag selects, a right-click opens no browser menu, a panel button starts placing a structure and
Escape stops it, the helper's switch turns it off, M mutes, Ctrl+1 and 1 bring a control group back, the
wheel zooms, and, with the menus on, the page opens on the main menu, Skirmish opens the setup, a button steps on, the fog button turns fog off and Start
begins the game; Escape opens the game menu, Save game saves, and the main menu's Load game brings the game back
at the saved tick. Browsers on the desktop keep Ctrl and a number for switching tabs, so there a control group may not
store; that is still open.

## Later

- Campaigns, which the main menu already lists as "not yet".
- More than one save, named; saving a replay to watch; loading a long game faster from a snapshot of the world
  (which would have to store the computer players' memory too).
- Control groups that a browser's own Ctrl+number shortcuts don't swallow; a rally point with several legs, or
  one that attack-moves.
- Icons when zoomed far out; the detailed models close in.
- Models for other settings, read from a setting pack instead of built into the program.
- Sounds and effects from a setting pack instead of made in code; music.
- Shot looks and impact effects from a setting pack's own art (sprites or models) on top of the three made in
  code; a scorch mark left on the ground where shells land; a launch sound for missiles.
- Units tilted to the slope they stand on; today a level box sinks into a hillside.
