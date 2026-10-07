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
| later: movement | Flow fields for groups plus local steering, slope costs per movement class (tracked, wheeled, legged, hover, air). | Planned |
| later: weapons | Ballistic and direct-fire projectiles in fixed point, blocked by terrain and units. | Planned |
| later: economy | Flow economy as a switchable module. | Planned |
| later: renderer | wgpu, native and WebGPU, reading only `events` and snapshots, with strategic zoom from day one. | Planned |

## Decisions so far

- One Rust core for both engines, native desktop plus WebAssembly web build (Ed, 7 Oct 2026).
- Integer and fixed-point maths everywhere in the simulation, enforced by clippy.
- Lockstep networking from the shared core; seekable replays from periodic state snapshots.
- First bets from `plans/rts-3d/improvements.md`: seekable replays with take-over, in-simulation automation and
  intent orders, headless self-play for balance.
