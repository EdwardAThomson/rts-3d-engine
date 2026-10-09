# Fog of war: design

Draft, 9 Oct 2026. The rules and names follow the Classic engine's fog (rts-engine `classic-sim` `vision`, from
playbooks `plans/rts/rules-world.md`) so both engines behave alike; what is new here is that hills block sight.
The code is `crates/sim3d/src/vision.rs`; drawing it is `crates/render3d/src/fog.rs` (docs/render.md, rule 13).

## Rules

1. **Off unless a game turns it on.** `World::set_fog(Some(FogRules))` turns it on, `None` off. `FogRules` has
   `hide` (fog hides enemy units; off means shroud only), `reveal_ticks` (45) and `start_explored` (no shroud,
   only fog). The skirmish turns it on with the defaults; the viewer's setup and `--fog 0` turn it off.
2. **Two arrays per player.** `explored` is set once a cell has ever been in sight; unexplored is **shroud**.
   `seen` counts the player's units that see the cell now; explored with none is **fog**; seen is **visible**.
3. **Vision is its own stat.** Each unit type has `vision` in cells, separate from weapon range. A unit sees every
   cell with `dx*dx + dy*dy <= r*r + r` from its own (a structure's counted from its footprint's edge), and always
   the eight cells round it.
4. **Hills block sight.** A cell counts as seen only if a straight line from the unit's eye (ground, altitude and
   height) to a point a little above the cell's ground clears the terrain (`Heightmap::line_of_sight`).
5. **Counted by reference.** What a unit sees depends only on its type and its cell, so it is worked out when the
   unit enters a cell and added; taken away when it leaves or dies. Adds and takes commute, so order never
   matters. An idea from OpenRA's and 0 A.D.'s shroud; the code is ours.
6. **Ghosts.** With `hide` on, each player remembers every enemy structure it has seen (kind, owner, place, health
   as last seen) and forgets it only once it sees the place again with the structure gone.
7. **Firing gives the shooter away.** The player fired at sees the cells round the shooter for `reveal_ticks`.
8. **Visibility is game state.** Idle units only pick enemies their side sees; a target that slips out of sight is
   dropped (a chase drives on to where it was going); an `Attack` order on an unseen target is ignored. The fog is
   hashed while it is on, so games without fog keep their old hashes.
9. **The computer plays by the same rules.** `ai3d` reads only what its side sees and remembers (docs/ai.md,
   rule 9).

## Later

- Radar and jammers, sight that grows with height, aircraft seeing over hills further.
- Fog settings from a setting pack and the skirmish setup (shroud only, start explored).
- Sharing sight between allies, once the engine has teams.
