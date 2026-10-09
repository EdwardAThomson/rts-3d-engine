# Architecture

Draft, 7 Oct 2026.

## Scope

True-3D real-time strategy in the tradition of Total Annihilation and Supreme Commander: units move freely over
heightmap terrain, terrain blocks sight and shots, projectiles fly real arcs, and the camera zooms from a single
unit to the whole map. 2D tile games belong to the Classic RTS Engine. The first scale target is a few hundred
units per side; measurements decide whether we go further.

## Three repositories

| Repository | Holds |
|---|---|
| `rts-core` | Genre-neutral code shared by both engines: integer maths, the seeded generator, the canonical state hash, the command log for replays, and later lockstep networking, the AI framework and setting-pack loading. |
| `rts-engine` | The Classic RTS Engine (2D tiles). |
| `rts-3d-engine` (this one) | Everything that needs continuous 3D space. |

`sim3d` depends on the public `rts-core` repository by git revision.

## Crates

| Crate | Job | Status |
|---|---|---|
| `sim3d` | The simulation: `space` (fixed-point `Vec3`, `SUB` = 256 sub-cell units per cell), `terrain` (corner heightmap, integer bilinear sampling, line of sight). | Started |
| `sim3d` | `movement` (movement classes from data with a slope limit, climb slowdown and altitude; one flow field per class and goal cell, shared by every unit sent there) and `world` (units, commands with a replayable log, `MoveEnded` events, the tick). | Started |
| `sim3d` | Steering in `world`: units are discs that push apart within their layer (ground or air), moving units shove idle ones aside, head-on pairs slide round each other, pushes respect slope limits, and a group packs round its goal. | Started |
| later: movement polish | Closest-reachable fallback for unreachable goals, facing and turn rates, formations. | Planned |
| `sim3d` | `weapon` (weapons from data; projectiles with a closed-form flight, straight for direct fire or a parabola for lobbed shots) and combat in `world`: unit types with health, `Attack` orders that close in until in range and in sight, shots stopped by the first hill or unit in the way, two-band splash, scatter from the seeded generator, a fixed combat order so mutual kills both land, owners, idle units picking the nearest enemy they can hit, half splash on their own side, and armour classes with each weapon's damage percent against each (0 = cannot hurt, never auto-picked), and `AttackMove` orders that halt to fight enemies met on the way and then drive on. | Started |
| later: combat | Turrets and facing, homing at aircraft, wrecks, faster unit lookups for many projectiles. | Planned |
| `sim3d` | `replay`: seekable replays. Recording plays the game once and keeps a copy of the world every N ticks; seeking starts from the nearest copy and plays forward. The world a seek returns takes new commands (take over from replay), and its log is itself a replay. Snapshots live in memory for now. | Started |
| `sim3d` | `economy` (docs/economy.md): a flow economy with resource kinds from data, a store per player with a capacity, units that produce every tick, and factories with queues that pay as they build. When spending outruns income, all of a player's factories slow down together. | Started |
| `sim3d` | Construction in `world/construction.rs`: builders place frames beside a site once it is clear and build them through the economy, other builders assist, and structures' footprints block ground movement (not aircraft) until destroyed. | Started |
| `sim3d` | Resource spots on the map that finished extractors yield, and wrecks that destroyed units leave and builders reclaim for their worth. A structure's wreck blocks its footprint until reclaimed; a unit's is driven over. | Started |
| `ai3d` | The computer opponent (docs/ai.md): reads `&World`, gives orders only through `World::command`, and finds builders, factories, income and fighters from the unit data. Builders keep every resource coming in and add factories; factories build builders then fighters; fighters gather, defend and attack in growing waves. Also a generic skirmish (`ai3d::skirmish`) and the headless `selfplay` runner for balance runs. | Started |
| `sim3d` | Fog of war in `vision` (docs/vision.md): per-player explored and seen cells, a `vision` stat per unit type, hills blocking sight, ghosts of enemy structures, firing revealing the shooter, and units only fighting what their side sees. Off unless a game turns it on; hashed while on. | Started |
| `sim3d` | Intent orders in `world/intent.rs` (docs/intent.md): patrols that fight on the way and turn round, factories that keep a count of each unit type their owner wants, factory rally points, and units that fall back to a point when badly hurt. All are state, so they hash and replay like any order. | Started |
| later: economy | Assisting factories, reclaiming frames, build priorities, upkeep. | Planned |
| `view3d` | The renderer's groundwork, with no GPU code (docs/view.md): a strategic-zoom camera that tilts from an angled close view to straight down over the whole map and zooms at the cursor, the terrain mesh at full or coarser detail, and ground picking against the simulation's own heights. Floating point is fine here: it only reads the state. | Started |
| `render3d` | The renderer (docs/render.md): the terrain, the art studio's glTF models for units and frames (turned the way they move, turrets to their targets, painted in their owner's colour), wrecks as their burnt, flattened models, shots in flight as tracers, shells or missiles with smoke trails in the looks the setting gives each unit kind, and a box for every spot, drawn with wgpu on `rts-platform` (the platform layer shared with the Classic engine, in the `rts-core` repository), sliding between ticks; tested offscreen. The `play3d` viewer opens on a main menu (Skirmish; Campaign and Load game shown as not yet; Quit) and a skirmish setup (seed, players, helper, watch, fog of war, with the map turning in a window) and ends on a game-over panel (winner, time, what each side built and lost, play again). It lets you play a side of the generic skirmish with the mouse (select, double-click for a kind, control groups, move, attack, attack-move, help build, reclaim, rally points) and a side panel (minimap, stock, build and production buttons, placing structures; a computer helper you can switch off runs your base), with flashes, fire, smoke and sound from the game's events, fog of war darkening the ground, models and minimap (enemies out of sight hidden, remembered structures kept), or watch computer players, on the desktop or in the browser (WebGPU or WebGL2). | Started |
| later: renderer | Sounds and effects from a setting pack, music. | Planned |

## Roadmap

Agreed with Ed on 7 Oct 2026; the order can change as we learn.

1. Done: terrain, movement and steering, combat with armour classes, attack-move, seekable replays with take-over.
2. Economy and building: flow economy, factories, builders, structures, resource spots and wrecks (done for a
   first game; polish later).
3. A simple AI and headless self-play runs for balance numbers (first version built: `ai3d`, docs/ai.md).
4. Intent orders: factory targets, fall-back rules, patrols that react (first version built: docs/intent.md).
5. Renderer: wgpu with strategic zoom, on the platform layer shared with the Classic engine (window, input, GPU
   setup, textures, text, audio), now the `rts-platform` crate in the `rts-core` repository. Started: `view3d`
   (camera, terrain mesh, picking) and `render3d` (drawing the art studio's models, playing a side with the mouse
   and a side panel for building and production, effects and sound, fog of war, on the desktop and in the browser).
6. Polish: turrets and facing, formations, a high-ground range bonus, craters, saving games to disk.

## Decisions so far

- One Rust core for both engines, native desktop plus WebAssembly web build (Ed, 7 Oct 2026).
- Integer and fixed-point maths everywhere in the simulation, enforced by clippy.
- Lockstep networking from the shared core; seekable replays from periodic state snapshots.
- First bets from `plans/rts-3d/improvements.md`: seekable replays with take-over, in-simulation automation and
  intent orders, headless self-play for balance.
