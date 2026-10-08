# Economy and production: design

Draft, 7 Oct 2026. The flow economy from `plans/rts-3d/inspirations.md` point 3, as a switchable module. The idea
is Total Annihilation's (resources as rates, not lump sums); the rules and numbers here are our own, and every
number is a starting value to tune by self-play runs.

## Goal

Players spend while they build, not before. Income and spending are both rates, so the question a player asks is
"can I afford to run three factories?" rather than "do I have 500?". When spending outruns income and the store is
empty, everything that spends slows down together and the game says so. Nothing is ever refused outright.

## Rules

1. **Resources come from data.** A setting lists its resource kinds; the engine never assumes how many (TA has
   two). Amounts are integers in milli-units, so a rate of 0.5 a tick is 500.
2. **Each player has a store per resource**, with a capacity. Income above capacity is lost. The store also
   reports the rate its builders ran at last tick, so the UI can say when the player is short.
3. **Producers.** A unit type may produce a fixed amount of each resource every tick (a generator). Resource
   spots are part of the map, each yielding a rate of one resource; a finished extractor yields every spot under
   its footprint on top of its own income. Frames produce nothing. Later: upkeep.
4. **Build power.** A unit type may have build power, the build time it adds each tick to whatever it is
   building. An item's cost and build time come from data; each tick, building at power `p` spends
   `cost * p / build_time` of each resource. What has been paid is always `cost * work / build_time` of the work
   done so far, so rounding never adds up and an item always costs exactly its cost.
5. **Stalls.** Each tick, for each player: income first, then every spender asks for its share. If the store
   cannot cover all requests for some resource, every spender of that player progresses at the same percentage,
   the lowest across resources (what is available divided by what was asked, in whole percent). Spenders are then
   paid in id order, each never taking more than is left, so a store never goes below empty. Later: build
   priorities.
6. **Factories** are units with build power, a list of what they can build, and a queue (repeat optional). The
   finished unit appears beside the factory, and heads for the factory's rally point if it has one (docs/intent.md).
   Later: exits that face a way.
7. **Construction.** A builder ordered to build a structure drives to a free cell beside the site, waits until no
   ground unit stands on it, places a frame (a unit at 1 health and no work done) and builds it from within one
   cell. The frame's health grows with the work done, on top of any damage it takes, and it does nothing (no
   income, no fire) until finished. Any builder that can build that type can assist a frame of its own side,
   adding its power. A builder that cannot reach its site gives up. A frame no one builds stays. Later: assisting
   factories, reclaiming frames, and moving idle units off a site.
8. **Structures** are units that never move. Their footprint, whole cells centred on the structure, blocks ground
   movement (flow fields and pushes go round it, and diagonal moves cannot squeeze past a corner) but not
   aircraft, and stops blocking when the structure is destroyed. Projectiles strike anywhere over the footprint.
   A site must be on the map, clear of other structures, and flat enough for the type's `max_rise`.
9. **Wrecks.** A finished unit whose type has a wreck worth leaves a wreck when destroyed; a frame leaves
   nothing. A builder next to a wreck reclaims it with its build power, gaining its worth the way building pays
   (exact in total), over half the type's build time. The wreck is gone once all of it is reclaimed. Income from
   reclaiming is capped by the store like any other. A structure's wreck keeps blocking the footprint it stood on
   until it is reclaimed, so ground units go round it and nothing is built there (Total Annihilation's rubble, our
   own code); a builder reclaims it from beside the footprint. A mobile unit's wreck is a low heap that blocks
   nothing: units drive over it, and the renderer lifts them over it. Ed asked for this on 8 Oct 2026, after seeing
   units drive through rubble. Later: reclaiming frames, wrecks that burn out.

## Order of work

1. Resources, stores, producers, stalls, and factories with queues (rules 1 to 6), with scenario tests. Built:
   `sim3d::economy` and `Command::Produce`/`ClearQueue` in `world`. Extractors wait for resource spots.
2. Builders, frames, assisting and structure footprints (rules 7 and 8). Built: `Command::Build`/`Assist` and
   `world/construction.rs`.
3. Resource spots on the map and extractors; wrecks and reclaim (rules 3 and 9). Built: `World::add_spot`,
   `Production::extracts` and `wreck`, `Command::Reclaim`.
