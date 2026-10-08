# The view: camera, terrain mesh and picking

Draft, 8 Oct 2026. Roadmap step 5 starts here. Everything the renderer (`crates/render3d`, docs/render.md) needs
that is not GPU code is in `crates/view3d`, so it is tested on its own and the renderer only has to draw.

## Rules

1. **An observer.** `view3d` reads the simulation and never changes it. Floating point is fine here; an order
   given with the mouse goes back as integer sub-cell units (`to_sub`) through `World::command`.
2. **View space is in cells.** `x` runs east and `y` south, as in the simulation, and `z` up. One cell of height
   is `HEIGHT_PER_CELL` height units, set equal to `SUB`, so slopes look as steep as the movement rules treat them.
3. **Strategic zoom.** One zoom value runs from 0 (four cells from the focus) to 1 (the whole map on screen), with
   the distance growing evenly in ratio. Close in, the camera looks 50 degrees below the horizon; as it pulls back
   it tilts until, fully out, it looks straight down with north up, like a minimap. Supreme Commander's camera is
   the inspiration; the numbers are our own starting values.
4. **Zoom at the cursor.** Zooming keeps the ground under the cursor where it is on screen, so a player zooms
   into what they point at. Panning moves by a share of what the screen shows, so it feels the same at every
   zoom, and keeps the focus on the map and on the ground.
5. **Terrain mesh.** One vertex per cell corner, two triangles per cell, anticlockwise from above so the renderer
   can cull undersides, with normals from the slope for lighting. Far views keep every 2nd, 4th or 8th corner
   (always including the far edges), chosen by the camera's distance.
6. **Picking tests the simulation's ground.** The ray from the eye through a pixel walks the map a quarter cell at
   a time against `Heightmap::sample` and then halves the last step, so an order lands where the simulation says
   the ground is, even where the drawn triangles cut a corner. The near side of a hill wins.

## Later

- Unit models from the art studio's glTF exports, with icons in place of models when zoomed far out.
- Terrain in chunks, so far chunks use coarse detail and off-screen ones are skipped.
- Picking units as well as ground; box selection.
- Camera limits from data (closest distance, close pitch) once the engine reads setting packs.
