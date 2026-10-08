//! The flow economy (docs/economy.md): resources are rates, not lump sums. Things are paid for while they are
//! built, a little each tick, and when spending outruns income every spender of that player slows down together
//! instead of anything being refused. The idea is Total Annihilation's; the rules and code are our own.
//!
//! Every amount is an integer in milli-units of a resource, so a rate of half a unit a tick is 500. How many
//! resources there are, and what each is called, comes from the setting; the engine only counts them.

use rts_core::hash::{Canon, CanonHasher};

/// What a unit type costs, builds and produces, read from data. The default is a unit that costs nothing,
/// builds nothing and produces nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Production {
    /// The cost of building one, per resource. A resource past the end of the list costs nothing.
    pub cost: Vec<i64>,
    /// The build work one takes. A builder adds its build power to it every tick.
    pub build_time: i64,
    /// The build work this unit adds each tick to whatever it is building.
    pub build_power: i64,
    /// The unit types this unit can produce, by index.
    pub builds: Vec<usize>,
    /// What this unit adds to its owner's store every tick, per resource.
    pub produces: Vec<i64>,
    /// Whether it is an extractor: a finished structure that also yields every resource spot under its footprint.
    pub extracts: bool,
    /// What the wreck it leaves when destroyed is worth, per resource. Empty leaves no wreck.
    pub wreck: Vec<i64>,
}

impl Production {
    pub fn cost_of(&self, resource: usize) -> i64 {
        self.cost.get(resource).copied().unwrap_or(0)
    }
}

/// What makes a unit type a structure: it never moves, and its footprint, a rectangle of whole cells centred on
/// its position, blocks ground movement. Read from data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Structure {
    /// Footprint size in cells, east-west and north-south.
    pub width: i32,
    pub depth: i32,
    /// The greatest difference between the heights of the footprint's corners on which it can be placed.
    pub max_rise: i32,
}

/// A resource spot on the map: an extractor whose footprint covers cell `(cx, cy)` yields `rate` of `resource`
/// every tick. Part of the map, set when a game is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    pub cx: i32,
    pub cy: i32,
    pub resource: usize,
    pub rate: i64,
}

impl Canon for Spot {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("cx", &self.cx)
            .field("cy", &self.cy)
            .field("rate", &self.rate)
            .field("resource", &(self.resource as u32))
            .end();
    }
}

/// What is left of a destroyed unit. Builders reclaim it for resources: its worth comes back as reclaim work is
/// done on it, the same way building pays out, and it is gone once all of it is reclaimed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wreck {
    /// Wrecks take ids from the same counter as units and projectiles.
    pub id: u32,
    /// The type of the unit it was, whose `wreck` worth it holds.
    pub kind: usize,
    pub pos: crate::space::Vec3,
    /// Reclaim work done so far.
    pub work: i64,
}

impl Canon for Wreck {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("id", &self.id)
            .field("kind", &(self.kind as u32))
            .field("pos", &self.pos)
            .field("work", &self.work)
            .end();
    }
}

/// The reclaim work a wreck of a type with this build time takes: half the build time, and at least one tick's
/// worth.
pub fn reclaim_time(build_time: i64) -> i64 {
    (build_time / 2).max(1)
}

/// A player's resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store {
    pub owner: u8,
    /// What the player has, per resource.
    pub amount: Vec<i64>,
    /// The most the player can hold, per resource. Income beyond it is lost.
    pub capacity: Vec<i64>,
    /// The percent of full speed the player's builders ran at last tick: 100 unless spending outran the store.
    pub rate: i64,
}

impl Canon for Store {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("amount", &self.amount)
            .field("capacity", &self.capacity)
            .field("owner", &self.owner)
            .field("rate", &self.rate)
            .end();
    }
}

/// One item in a factory's queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    /// The unit type to build.
    pub kind: usize,
    /// Whether to queue it again once it is built.
    pub repeat: bool,
}

impl Canon for Job {
    fn canon(&self, w: &mut CanonHasher) {
        w.object().field("kind", &(self.kind as u32)).field("repeat", &self.repeat).end();
    }
}

/// How much of a resource costing `cost` has been paid once `work` of `time` is done. Paying this way, and
/// never by summing rounded steps, means an item always costs exactly its cost.
pub fn paid(cost: i64, work: i64, time: i64) -> i64 {
    cost * work / time
}

/// The most work, starting from `work`, that `budget` more of a resource costing `cost` pays for.
pub fn affordable(cost: i64, work: i64, time: i64, budget: i64) -> i64 {
    if cost == 0 {
        return time;
    }
    // The largest w with cost * w / time <= paid + budget.
    ((paid(cost, work, time) + budget + 1) * time - 1) / cost
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paying_by_work_done_always_adds_up_to_the_cost() {
        let (cost, time) = (1000, 7);
        let mut total = 0;
        for work in 0..time {
            total += paid(cost, work + 1, time) - paid(cost, work, time);
        }
        assert_eq!(total, cost);
    }

    #[test]
    fn affordable_work_is_the_most_a_budget_pays_for() {
        let (cost, time) = (1000, 7);
        for budget in 0..400 {
            let w = affordable(cost, 2, time, budget);
            assert!(paid(cost, w, time) - paid(cost, 2, time) <= budget);
            assert!(paid(cost, w + 1, time) - paid(cost, 2, time) > budget);
        }
        assert_eq!(affordable(0, 3, time, 0), time, "free things never wait");
    }
}
