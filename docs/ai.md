# Computer opponent and self-play: design

Draft, 8 Oct 2026. Roadmap step 3: a simple opponent, and headless games between computer players whose results
tune the numbers. The shape follows the Classic engine's opponent (`rts-engine`, `crates/classic-ai`, and
playbooks `plans/rts/ai-opponent.md`); the rules and numbers here are our own and are starting values for
self-play runs to tune.

## Rules

1. **A player without a mouse.** The opponent reads the world through `&World` and acts only through
   `World::command`, so its orders are checked, queued and logged like a human's. A replay of the command log
   plays its games back with the opponent switched off. Its memory lives in the `Ai` value, outside the state
   hash.
2. **Deterministic.** Integer maths, units in id order, no clock and no randomness of its own. Two opponents
   playing each other give the same hash on every run.
3. **Roles come from data.** It knows no unit by name. A mobile unit with build power that can build structures
   is a builder; a structure that builds mobile units is a factory; a structure that makes a resource, or an
   extractor while the map has spots, is income; a mobile unit with a weapon is a fighter. Resources are counted,
   never assumed.
4. **Base.** Each think, every idle builder takes the first that applies: start a new structure while fewer than
   `projects` are under way (income for any resource with none, then a first factory, then income for the
   scarcest resource that is short or for the scarcest of all while spending stalls, then an extractor on any
   free spot on its side of the map, then another factory while every resource is plentiful); help finish the
   nearest frame; reclaim the nearest wreck within `reclaim_radius` of home. It moves its own idle units off
   sites its builders wait on.
5. **Sites.** An extractor goes over a free spot no nearer to an enemy structure (other than an extractor) than to
   home, nearest home first. Other structures take the first good site in rings round home, furthest from the
   rally point first so the base grows away from the enemy: flat enough, off the spots, clear of units and plans,
   with a free cell all round so the base never walls itself in.
6. **Factories** keep their queues topped up: builders until the player has `builders`, then fighters, taking
   turns between the kinds each factory can build.
7. **Army.** New fighters gather at a rally point between home and the nearest enemy. Enemy fighters near a
   structure draw out every fighter at home. When enough have gathered they set out as a wave, attack-moving on
   the nearest enemy structure (to a free cell beside it) and then on whatever enemy is nearest each of them. A
   wave beaten below `retreat_percent` of its starting size comes home. Each wave is bigger than the last, up to a
   cap.
8. **Defeat.** A player with no structure, frame or builder left has lost; fighters alone cannot rebuild. The
   last player standing wins.

## Self-play

`ai3d::skirmish` is a generic test game: a 64 by 64 map of rolling hills from a seed, mirrored so every corner
start is the same, two to four players with one builder each, ore spots near each base, in the empty corners
and round the middle, and six generic unit types (builder, generator, extractor, factory, tank, artillery). It
stands in for a setting pack until the 3D engine reads packs.

`selfplay` runs computer players on it with no window:

```bash
cargo run --release -p ai3d --bin selfplay -- --seed 1 --ticks 36000 --every 3000
cargo run --release -p ai3d --bin selfplay -- --seeds 1..20
cargo run --release -p ai3d --bin selfplay -- --seed 1 --idle 1    # player 1 does nothing
```

Each game prints the winner and when, the final state hash, and what each player built and lost; a range of
seeds adds the win counts and the mean length. Those numbers are what balance changes are judged by.

## First runs

Ten seeds of two opponents on the skirmish, up to 54,000 ticks each (8 Oct 2026): 7 games won, 3 with no
winner, mean length about 26,600 ticks; the run takes about a minute on one core. The south-east corner won 6
of the 7. With the seats swapped (seeds 1 to 8, up to 30,000 ticks) it won 6 of 8, so the bias follows the
corner, not the player number. Likely causes to look at next: factories always release units to the south, and the
fixed tie-break orders in flow fields and steering are not mirror images. A fair mirror matters before any
balance number is trusted.

## Later

- Fog of war: read only what its own units see, once the engine has fog.
- Settings from data (difficulty levels) instead of `Settings::normal()`.
- Assisting factories, which the economy does not yet allow; aircraft and naval roles; choosing fighter kinds by
  what the enemy fields (counters by armour class).
- Reachability: today it assumes its builders can reach every site near home, which holds on the skirmish map
  but not on maps with water or cliffs.
