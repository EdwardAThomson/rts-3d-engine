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
4. **Base.** Each think, every idle builder takes the first that applies: reclaim a structure's wreck lying on a
   resource spot on its side of the map (it blocks the spot until it is gone), unless another builder is on it;
   start a new structure while fewer than
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
9. **Fog of war.** With fog on it knows only what its side sees now, plus the enemy structures it remembers
   (`known`). With no enemy known it guesses the enemy is at its own home mirrored through the middle of the map:
   the rally point leans that way, and a wave sent out with nothing to attack scouts (`scout`), heading for points
   on a four-cell grid that are shroud first, then fog, nearest that guess. Bases it has never seen draw no
   attacks and don't affect its site choices.

## Self-play

`ai3d::skirmish` is a generic test game: a 64 by 64 map of rolling hills from a seed, mirrored so every corner
start is the same, two to four players with one builder each, ore spots near each base, in the empty corners
and round the middle, and six generic unit types (builder, generator, extractor, factory, tank, artillery). It
stands in for a setting pack until the 3D engine reads packs. Each start is nudged up to a quarter of a cell by
the seed, the same way whoever sits there, so a mirrored game is not settled by which way an exact tie breaks.
`skirmish_turned` turns the seats, so player `p` starts in corner `(p + turn) % players`.

`selfplay` runs computer players on it with no window:

```bash
cargo run --release -p ai3d --bin selfplay -- --seed 1 --ticks 36000 --every 3000
cargo run --release -p ai3d --bin selfplay -- --seeds 1..20
cargo run --release -p ai3d --bin selfplay -- --seed 1 --idle 1    # player 1 does nothing
cargo run --release -p ai3d --bin selfplay -- --seeds 1..10 --fog 0  # without fog of war
```

Each seed is played once per seat arrangement, so every player gets every corner (`--turn N` plays one). Each
game prints the winner, their corner and when, the final state hash, and what each player built and lost. The
summary counts wins by player and by corner and gives the mean length. Wins by player are what balance changes
are judged by; wins by corner show whether the map or the engine favours a place.

## Fair corners

The first runs (8 Oct 2026) had one corner winning almost every decisive game, whichever player sat there.
Mirrored test games showed where it came from:

- Moves and shots rounded towards negative infinity, so a unit heading north-west covered a little more ground
  each tick than its mirror image heading south-east, and shots landed differently. They now round towards the
  start point.
- Flow fields broke ties between equally cheap steps by a fixed compass order, so a group heading one way went
  diagonal first and its mirror image went straight first. Ties now go to the step whose cell is nearest the
  goal in a straight line.
- A unit exactly on a cell edge counted as in the cell on the far side for one direction only. It now steers from
  whichever touching cell is cheaper to reach the goal from.
- The opponent laid out its base from a fixed row order and an off-centre site for even sizes; it now measures
  every site from home to the site's centre, so bases in opposite corners are mirror images.

`sim3d`'s movement and combat tests now check that a move, a group move and a shot each stay exact mirror images
of their mirror-image twins. The skirmish nudges each start by the seed and self-play turns the seats, so the
few ties that still break one way (exact ties at a corner, the fixed exit side of factories) cannot settle a
whole run. Thirty seeds, both seatings, up to 30,000 ticks: wins by corner 30 to 26, by player 30 to 26, 4 with
no winner, mean length about 13,200 ticks.

With fog of war on (9 Oct 2026), ten seeds, both seatings: 17 of 20 games decided, mean length about 17,900
ticks; the same seeds without fog (`--fog 0`): 18 of 20, about 14,200 ticks. Finding the enemy costs the first
wave some time; the undecided games are the same slow economic stand-offs as without fog.

With facing and turrets (10 Oct 2026): the builder and tank turn their bodies at 48 a tick and the artillery at 32
(of 4,096 to a turn), the tank's turret at 96 and the artillery's at 40, so flanking slow guns pays. Forty seeds,
both seatings, with fog: 71 of 80 games decided, wins by corner 36 to 35 and by player 41 to 30, mean length
about 16,500 ticks. `sim3d`'s facing tests check that turning and aiming stay exact mirror images too.

## Later

- Sending a cheap unit to scout early, instead of the first wave doing it.
- Settings from data (difficulty levels) instead of `Settings::normal()`.
- Assisting factories, which the economy does not yet allow; aircraft and naval roles; choosing fighter kinds by
  what the enemy fields (counters by armour class).
- Reachability: today it assumes its builders can reach every site near home, which holds on the skirmish map
  but not on maps with water or cliffs.
