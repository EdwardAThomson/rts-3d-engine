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
3. **Producers.** A unit type may produce a fixed amount of each resource every tick (a generator, an extractor
   on a resource spot). Extractors only produce while standing on a resource spot from the map. Later: upkeep.
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
   finished unit appears beside the factory. Later: rally points and exits that face a way.
7. **Construction.** A builder ordered to build a structure drives to the site, places a frame (a unit at 1 health
   and 0% progress), and builds it. Health grows with progress. Any builder can assist any frame or factory
   job of its own side, adding its power. A frame no one builds stays. Reclaiming frames and wrecks comes later.
8. **Structures** are units that do not move. Their footprint blocks ground movement, so flow fields treat it as
   impassable.

## Order of work

1. Resources, stores, producers, stalls, and factories with queues (rules 1 to 6), with scenario tests. Built:
   `sim3d::economy` and `Command::Produce`/`ClearQueue` in `world`. Extractors wait for resource spots.
2. Builders, frames, assisting and structure footprints (rules 7 and 8).
3. Resource spots on the map and extractors; wrecks and reclaim.
