//! Formations: a group ordered to a point as one goes there in ranks, facing the way it went, and its members
//! arrive together.
//!
//! The order lays the group out in rows across the way from the group's middle to the point, a little wider than
//! deep, with the shortest-ranged armed units in front, longer-ranged ones behind them and unarmed units at the
//! back. Each member heads for its own place in that layout. While the group is under way every member drives at
//! the slowest member's speed, scaled by how much of the way it has left compared with whoever has the most left,
//! so they close up and arrive together. Each follows the route to the point shifted out to its own place, so
//! the group keeps its shape on the way and doesn't funnel into one file. A member that halts to fight on an
//! attack-move doesn't hold the rest back. On arrival each turns to face the way the group went. A member given any
//! other order leaves the formation.
//!
//! Total Annihilation and Supreme Commander move groups in formation; the layout and pacing rules here are our own.

use super::World;
use crate::movement::FlowField;
use crate::space::{SUB, bearing, rotate};
use rts_core::imath::isqrt;
use std::collections::BTreeMap;

/// A formation's pacing for a tick: the slowest member's speed, the most way any member has left, and each member
/// under way with the way it has left.
type Pacing = (i32, i64, Vec<(usize, i64)>);

/// The space between neighbours in a formation, as a multiple of the widest member's radius, in halves.
const SPACING_HALVES: i32 = 5;

impl World {
    pub(super) fn order_formation(&mut self, units: &[u32], x: i32, y: i32, hunt: bool) {
        let mut members: Vec<usize> = units
            .iter()
            .filter_map(|&id| self.index_of(id))
            .filter(|&i| self.types[self.units[i].kind].structure.is_none() && self.units[i].build.is_none())
            .collect();
        members.sort_unstable();
        members.dedup();
        let Some(&first) = members.first() else { return };
        let (x, y) = self.onto_map(x, y);
        // The way from the group's middle to the point, without dividing, so a mirrored group faces exactly the
        // opposite way.
        let n = members.len() as i64;
        let (mut dx, mut dy) = members.iter().fold((n * i64::from(x), n * i64::from(y)), |(dx, dy), &i| {
            (dx - i64::from(self.units[i].pos.x), dy - i64::from(self.units[i].pos.y))
        });
        while dx.abs().max(dy.abs()) > i64::from(i32::MAX / 2) {
            (dx, dy) = (dx / 2, dy / 2);
        }
        let facing = bearing(dx as i32, dy as i32).unwrap_or(self.units[first].facing);

        // Ranks: short-ranged fighters in front, then longer-ranged, then the unarmed; ties by id.
        let types = &self.types;
        members.sort_by_key(|&i| {
            let u = &self.units[i];
            (types[u.kind].weapon.as_ref().map_or(i32::MAX, |w| w.range), u.id)
        });
        let widest = members.iter().map(|&i| types[self.units[i].kind].movement.radius).max().unwrap_or(1);
        let space = widest * SPACING_HALVES / 2;
        let count = members.len() as i32;
        let mut cols = 1;
        while cols * cols < 2 * count {
            cols += 1;
        }
        let cols = cols.min(count);
        let rows = (count + cols - 1) / cols;

        let group = self.next_id;
        self.next_id += 1;
        for (n, &i) in members.iter().enumerate() {
            let (row, col) = (n as i32 / cols, n as i32 % cols);
            let in_row = cols.min(count - row * cols);
            // Across and back from the point, in a frame facing north: east is across, south is back.
            let across = (2 * col - (in_row - 1)) * space / 2;
            let back = (2 * row - (rows - 1)) * space / 2;
            let (ox, oy) = rotate(across, back, facing);
            // Kept a step inside the map's edges, which a mirror image keeps too.
            let inside = |v: i32, cells: i32| v.clamp(1, cells * SUB - 1);
            let slot = (inside(x + ox, self.map.width()), inside(y + oy, self.map.height()));
            // A place the member can't get to (inside a structure, up a cliff) gives way to the point itself.
            let ok = self.reachable(i, slot) && self.reachable(i, (x, y));
            let goal = if ok { slot } else { (x, y) };
            let u = &mut self.units[i];
            (u.target, u.chase, u.hunt, u.plan, u.assist) = (None, false, hunt, None, None);
            (u.reclaim, u.patrol) = (None, None);
            self.set_goal(i, goal.0, goal.1);
            let u = &mut self.units[i];
            (u.group, u.face) = (Some(group), Some(facing));
            // On the way it keeps to its lane: the route to the point, shifted out to its place.
            u.lane = ok.then_some((slot.0 - x, slot.1 - y));
        }
    }

    /// Whether unit `i`'s class can get from where it stands to `point`.
    fn reachable(&mut self, i: usize, point: (i32, i32)) -> bool {
        let u = &self.units[i];
        let (kind, here) = (u.kind, super::cell_of(&self.map, (u.pos.x, u.pos.y)));
        let cell = super::cell_of(&self.map, point);
        let (map, blocked, types) = (&self.map, &self.blocked, &self.types);
        let field = self
            .fields
            .entry((kind, cell))
            .or_insert_with(|| FlowField::build(map, blocked, &types[kind].movement, cell));
        field.cost(here).is_some()
    }

    /// How fast each unit may drive this tick, by index: its class's speed, or less for a member of a formation
    /// under way. A member whose move is over leaves its formation.
    pub(super) fn paces(&mut self) -> Vec<i32> {
        let mut paces: Vec<i32> = self.units.iter().map(|u| self.types[u.kind].movement.speed).collect();
        let mut groups: BTreeMap<u32, Pacing> = BTreeMap::new();
        for (i, u) in self.units.iter_mut().enumerate() {
            let Some(group) = u.group else { continue };
            let Some((gx, gy)) = u.goal else {
                (u.group, u.lane) = (None, None);
                continue;
            };
            let entry = groups.entry(group).or_insert((i32::MAX, 0, Vec::new()));
            entry.0 = entry.0.min(paces[i]);
            if u.hunt && u.target.is_some() {
                continue;
            }
            let (dx, dy) = (i64::from(gx - u.pos.x), i64::from(gy - u.pos.y));
            let left = isqrt((dx * dx + dy * dy) as u64) as i64;
            entry.1 = entry.1.max(left);
            entry.2.push((i, left));
        }
        for (speed, most, moving) in groups.into_values() {
            for (i, left) in moving {
                if most > 0 {
                    paces[i] = ((i64::from(speed) * left / most) as i32).max(1);
                }
            }
        }
        paces
    }
}
