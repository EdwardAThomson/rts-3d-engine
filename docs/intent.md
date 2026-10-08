# Intent orders: design

Draft, 8 Oct 2026. Roadmap step 4, from `plans/rts-3d/improvements.md` (in-simulation automation): orders that
say what a player wants rather than one thing to do now, which the simulation keeps carrying out by itself.
Total Annihilation's patrol and Supreme Commander's repeating factory orders are the inspiration; the rules here
are our own.

## Rules

1. **Intents are state.** They live on the unit, are part of the state hash, are given by commands like any
   other order, and so replay exactly and look the same to every player in a lockstep game. Nothing outside the
   simulation runs them.
2. **Patrol** (`Command::Patrol`). The unit attack-moves to the point; when it gets there it attack-moves back to
   where the order was given, and so on. It stops to fight enemies met on the way, as on any attack-move. Any
   other order ends the patrol, and so does a leg it cannot reach.
3. **Keep** (`Command::Keep`), a standing rule for a factory. Whenever its queue is empty and its owner has fewer
   units of a type than the rule asks, counting those queued in any of the owner's factories, it queues one more.
   Rules are checked in the order given. Several factories can share a target without overshooting it. A count
   of 0 drops the rule. Manual `Produce` orders still work and go first.
4. **Fall back** (`Command::FallBack`), a standing rule for a unit. When damage takes its health below a percent
   of its maximum, it drops what it is doing (target, attack-move, patrol, build) and moves to the point. It acts
   when health crosses the line, not on every hit, so a unit sent back into the fight below the line stays
   there. A percent of 0 drops the rule.

## Order of a tick

Fall-back rules run right after damage, before anyone moves. Patrols turn round right after moves end. Factory
rules run at the start of the economy step, before anything is paid for.

## Later

- Patrols with more than two points, and guard (follow a unit and fight what attacks it).
- Repair, so a fallen-back unit can be patched up and sent back; then a rule to return once healed.
- Keep rules for structures (a builder that keeps N extractors), and priorities between rules.
- Formations that hold together on a patrol.
