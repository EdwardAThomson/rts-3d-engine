//! Intent orders (docs/intent.md): orders that say what a player wants rather than one thing to do now, and that
//! the simulation keeps carrying out on its own. They are part of the state, so they are hashed, replayed and
//! seen the same way by every player in a lockstep game. The idea comes from plans/rts-3d/improvements.md
//! (in-simulation automation); Total Annihilation's patrol and Supreme Commander's factory loops are the
//! inspiration, and the rules are our own.
//!
//! - Patrol: attack-move to a point, then back to where the order was given, and so on.
//! - Keep: a factory with an empty queue builds whatever its owner has fewer of than it wants.
//! - Fall back: a unit hurt below a share of its health leaves the fight for a point.

use super::{Job, MoveEnd, World};

impl World {
    pub(super) fn order_patrol(&mut self, unit: u32, x: i32, y: i32) {
        let Some(i) = self.index_of(unit) else { return };
        if self.types[self.units[i].kind].structure.is_some() {
            return;
        }
        let u = &mut self.units[i];
        let here = (u.pos.x, u.pos.y);
        (u.target, u.chase, u.hunt, u.plan, u.assist, u.reclaim) = (None, false, true, None, None, None);
        u.patrol = Some(here);
        self.set_goal(i, x, y);
    }

    pub(super) fn order_keep(&mut self, unit: u32, kind: usize, count: u32) {
        let Some(i) = self.index_of(unit) else { return };
        let builds = &self.types[self.units[i].kind].production.builds;
        if !builds.contains(&kind) || self.types[kind].structure.is_some() {
            return;
        }
        let keep = &mut self.units[i].keep;
        keep.retain(|&(k, _)| k != kind);
        if count > 0 {
            keep.push((kind, count));
        }
    }

    /// Units on patrol that have just arrived head for the other end, still fighting on the way.
    pub(super) fn turn_patrols(&mut self, ended: &[(usize, MoveEnd)]) {
        for &(i, reason) in ended {
            let u = &self.units[i];
            if reason != MoveEnd::Arrived {
                continue;
            }
            let (Some(next), Some(here)) = (u.patrol, u.rest) else { continue };
            self.units[i].patrol = Some(here);
            self.units[i].hunt = true;
            self.set_goal(i, next.0, next.1);
        }
    }

    /// Every factory with standing targets and nothing queued, in id order, queues one of the first type its
    /// owner has fewer of than it wants, counting units already queued in that owner's factories.
    pub(super) fn keep_stocked(&mut self) {
        for i in 0..self.units.len() {
            let f = &self.units[i];
            if f.keep.is_empty() || !f.queue.is_empty() || f.build.is_some() {
                continue;
            }
            let owner = f.owner;
            let have = |kind: usize| {
                self.units
                    .iter()
                    .filter(|u| u.owner == owner)
                    .map(|u| usize::from(u.kind == kind) + u.queue.iter().filter(|j| j.kind == kind).count())
                    .sum::<usize>()
            };
            let short = f.keep.iter().find(|&&(kind, count)| have(kind) < count as usize).map(|&(kind, _)| kind);
            if let Some(kind) = short {
                self.units[i].queue.push(Job { kind, repeat: false });
            }
        }
    }

    /// Units with a fall-back rule whose health this tick's damage took below the line drop what they are doing
    /// and move to their fall-back point.
    pub(super) fn fall_back(&mut self, hurt: Vec<(u32, i32)>) {
        for (id, before) in hurt {
            let Some(i) = self.index_of(id) else { continue };
            let Some((percent, x, y)) = self.units[i].fall_back else { continue };
            let max = i64::from(self.types[self.units[i].kind].max_health);
            let line = |health: i32| i64::from(health) * 100 < max * i64::from(percent);
            if line(before) || !line(self.units[i].health) {
                continue;
            }
            let u = &mut self.units[i];
            (u.target, u.chase, u.hunt, u.plan, u.assist, u.reclaim, u.patrol) =
                (None, false, false, None, None, None, None);
            self.set_goal(i, x, y);
        }
    }
}
