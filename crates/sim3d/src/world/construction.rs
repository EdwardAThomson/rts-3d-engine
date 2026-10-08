//! Builders and structures (docs/economy.md, rules 7 and 8). A builder ordered to build a structure drives next
//! to the site and, once no ground unit stands on it, places a frame: a unit at 1 health that does nothing until
//! it is finished. Builders next to a frame of their own side add their build power to it, paid for through the
//! economy like a factory's work. Structures never move, and their footprints block ground movement, so flow
//! fields and pushes go round them. Total Annihilation's nanolathe-and-frame idea, our own code.

use super::{Event, SUB, World, centre};

/// How far beyond touching a builder can reach the thing it builds, in sub-cell units.
const BUILD_REACH: i32 = SUB;

impl World {
    /// Rebuild the blocked cells from the structures standing now, and every flow field with them.
    pub(super) fn reblock(&mut self) {
        let (w, h) = (self.map.width(), self.map.height());
        let mut blocked = vec![false; (w * h) as usize];
        for u in &self.units {
            if let Some((x0, y0, x1, y1)) = self.footprint_cells(u.kind, u.pos.x, u.pos.y) {
                for cy in y0.max(0)..y1.min(h) {
                    for cx in x0.max(0)..x1.min(w) {
                        blocked[(cy * w + cx) as usize] = true;
                    }
                }
            }
        }
        self.blocked = blocked;
        self.fields.clear();
        for i in 0..self.units.len() {
            if let Some((x, y)) = self.units[i].goal {
                let rest = self.units[i].rest;
                self.set_goal(i, x, y);
                self.units[i].rest = rest;
            }
        }
    }

    /// The cells a structure of type `kind` centred at `(x, y)` covers, as `[x0, x1) x [y0, y1)`.
    pub(super) fn footprint_cells(&self, kind: usize, x: i32, y: i32) -> Option<(i32, i32, i32, i32)> {
        let s = self.types[kind].structure?;
        let (x0, y0) = ((x - s.width * SUB / 2).div_euclid(SUB), (y - s.depth * SUB / 2).div_euclid(SUB));
        Some((x0, y0, x0 + s.width, y0 + s.depth))
    }

    /// Half the size of a unit of type `kind` along an axis (0 east-west, 1 north-south): half its footprint for
    /// a structure, its radius otherwise.
    pub(super) fn half_extent(&self, kind: usize, axis: usize) -> i32 {
        match self.types[kind].structure {
            Some(s) => [s.width, s.depth][axis] * SUB / 2,
            None => self.types[kind].movement.radius,
        }
    }

    /// The ground distance from `(x, y)` to a unit's body: its footprint for a structure, its disc otherwise. 0
    /// means the point is on or inside it.
    pub(super) fn gap(&self, u: &super::Unit, x: i32, y: i32) -> i64 {
        let (dx, dy) = (i64::from((x - u.pos.x).abs()), i64::from((y - u.pos.y).abs()));
        if self.types[u.kind].structure.is_some() {
            let (hx, hy) = (i64::from(self.half_extent(u.kind, 0)), i64::from(self.half_extent(u.kind, 1)));
            let (ox, oy) = ((dx - hx).max(0), (dy - hy).max(0));
            return i64::from(rts_core::imath::isqrt((ox * ox + oy * oy) as u64) as i32);
        }
        let d = i64::from(rts_core::imath::isqrt((dx * dx + dy * dy) as u64) as i32);
        (d - i64::from(self.types[u.kind].movement.radius)).max(0)
    }

    /// Whether cell `(cx, cy)` is under a structure's footprint (cells off the map are not).
    pub fn is_blocked(&self, cx: i32, cy: i32) -> bool {
        let (w, h) = (self.map.width(), self.map.height());
        (0..w).contains(&cx)
            && (0..h).contains(&cy)
            && self.blocked.get((cy * w + cx) as usize).copied().unwrap_or(false)
    }

    /// Whether builder `i` is close enough to build unit `t`.
    pub(super) fn in_reach(&self, i: usize, t: usize) -> bool {
        let me = &self.units[i];
        let reach = i64::from(self.types[me.kind].movement.radius + BUILD_REACH);
        self.gap(&self.units[t], me.pos.x, me.pos.y) <= reach
    }

    /// Where unit `i` should head to reach unit `t`: the target itself, or for a structure the centre of the free
    /// cell next to its footprint nearest to `i` (ties to the lowest row, then column).
    pub(super) fn towards(&self, i: usize, t: usize) -> (i32, i32) {
        let (me, them) = (&self.units[i], &self.units[t]);
        match self.footprint_cells(them.kind, them.pos.x, them.pos.y) {
            Some(cells) => self.beside(cells, me.pos.x, me.pos.y).unwrap_or((them.pos.x, them.pos.y)),
            None => (them.pos.x, them.pos.y),
        }
    }

    /// The centre of the free cell bordering `cells` nearest to `(x, y)`.
    fn beside(&self, (x0, y0, x1, y1): (i32, i32, i32, i32), x: i32, y: i32) -> Option<(i32, i32)> {
        let (w, h) = (self.map.width(), self.map.height());
        let mut best: Option<(i64, i32, i32)> = None;
        for cy in (y0 - 1).max(0)..=y1.min(h - 1) {
            for cx in (x0 - 1).max(0)..=x1.min(w - 1) {
                let inside = (x0..x1).contains(&cx) && (y0..y1).contains(&cy);
                if inside || self.blocked.get((cy * w + cx) as usize).copied().unwrap_or(false) {
                    continue;
                }
                let (px, py) = centre((cx, cy));
                let (dx, dy) = (i64::from(px - x), i64::from(py - y));
                let d2 = dx * dx + dy * dy;
                if best.is_none_or(|(b, _, _)| d2 < b) {
                    best = Some((d2, cy, cx));
                }
            }
        }
        best.map(|(_, cy, cx)| centre((cx, cy)))
    }

    /// Whether a structure of type `kind` can stand with its north-west cell at `(cx, cy)`: on the map, clear of
    /// other structures, and with corners no further apart in height than it allows. Ground units standing there
    /// don't count; a builder waits for them to leave.
    pub fn site_ok(&self, kind: usize, cx: i32, cy: i32) -> bool {
        let Some(s) = self.types[kind].structure else { return false };
        let (w, h) = (self.map.width(), self.map.height());
        if cx < 0 || cy < 0 || cx + s.width > w || cy + s.depth > h {
            return false;
        }
        let mut heights = Vec::new();
        for y in cy..cy + s.depth {
            for x in cx..cx + s.width {
                if self.blocked.get((y * w + x) as usize).copied().unwrap_or(false) {
                    return false;
                }
            }
        }
        for y in cy..=cy + s.depth {
            for x in cx..=cx + s.width {
                heights.push(self.map.corner_height(x, y));
            }
        }
        let (lo, hi) = (heights.iter().min().copied(), heights.iter().max().copied());
        matches!((lo, hi), (Some(lo), Some(hi)) if hi - lo <= s.max_rise)
    }

    fn can_build(&self, i: usize, kind: usize) -> bool {
        let p = &self.types[self.units[i].kind].production;
        self.units[i].build.is_none() && p.build_power > 0 && p.builds.contains(&kind)
    }

    pub(super) fn order_build(&mut self, unit: u32, kind: usize, cx: i32, cy: i32) {
        let Some(i) = self.index_of(unit) else { return };
        if !self.can_build(i, kind) || self.types[kind].structure.is_none() || !self.site_ok(kind, cx, cy) {
            return;
        }
        let u = &mut self.units[i];
        (u.target, u.chase, u.hunt, u.assist, u.goal) = (None, false, false, None, None);
        (u.reclaim, u.patrol) = (None, None);
        u.plan = Some((kind, cx, cy));
    }

    pub(super) fn order_assist(&mut self, unit: u32, target: u32) {
        let (Some(i), Some(t)) = (self.index_of(unit), self.index_of(target)) else { return };
        let (me, frame) = (&self.units[i], &self.units[t]);
        if frame.build.is_none() || frame.owner != me.owner || !self.can_build(i, frame.kind) {
            return;
        }
        let u = &mut self.units[i];
        (u.target, u.chase, u.hunt, u.plan, u.goal) = (None, false, false, None, None);
        (u.reclaim, u.patrol) = (None, None);
        u.assist = Some(target);
    }

    pub(super) fn order_reclaim(&mut self, unit: u32, wreck: u32) {
        let Some(i) = self.index_of(unit) else { return };
        let u = &self.units[i];
        if u.build.is_some() || self.types[u.kind].production.build_power == 0 || self.wreck_index(wreck).is_none() {
            return;
        }
        let u = &mut self.units[i];
        (u.target, u.chase, u.hunt, u.plan, u.assist, u.goal) = (None, false, false, None, None, None);
        (u.reclaim, u.patrol) = (Some(wreck), None);
    }

    fn wreck_index(&self, id: u32) -> Option<usize> {
        self.wrecks.binary_search_by_key(&id, |w| w.id).ok()
    }

    /// Whether builder `i` is close enough to reclaim wreck `w`.
    pub(super) fn near_wreck(&self, i: usize, w: usize) -> bool {
        let (me, wreck) = (&self.units[i], &self.wrecks[w]);
        let reach = i64::from(self.types[me.kind].movement.radius + BUILD_REACH);
        i64::from(me.pos.ground_distance(wreck.pos)) <= reach
    }

    /// Every builder, in id order, works towards its plan or the frame it assists: it goes next to the site,
    /// places the frame when the site is clear, and stops once it is close enough to build.
    pub(super) fn construct(&mut self, events: &mut Vec<Event>) {
        for i in 0..self.units.len() {
            if let Some((kind, cx, cy)) = self.units[i].plan {
                self.follow_plan(i, kind, cx, cy, events);
            } else if let Some(id) = self.units[i].reclaim {
                match self.wreck_index(id) {
                    None => self.units[i].reclaim = None,
                    Some(w) if self.near_wreck(i, w) => self.units[i].goal = None,
                    Some(w) => {
                        if self.units[i].goal.is_none() {
                            let p = self.wrecks[w].pos;
                            self.set_goal(i, p.x, p.y);
                        }
                    }
                }
            } else if let Some(target) = self.units[i].assist {
                match self.index_of(target).filter(|&t| self.units[t].build.is_some()) {
                    None => self.units[i].assist = None,
                    Some(t) if self.in_reach(i, t) => self.units[i].goal = None,
                    Some(t) => {
                        if self.units[i].goal.is_none() {
                            let (x, y) = self.towards(i, t);
                            self.set_goal(i, x, y);
                        }
                    }
                }
            }
        }
    }

    fn follow_plan(&mut self, i: usize, kind: usize, cx: i32, cy: i32, events: &mut Vec<Event>) {
        if !self.site_ok(kind, cx, cy) {
            // Something else was built there first.
            (self.units[i].plan, self.units[i].goal) = (None, None);
            return;
        }
        let s = self.types[kind].structure.unwrap_or_else(|| unreachable!("checked when ordered"));
        let (x, y) = (cx * SUB + s.width * SUB / 2, cy * SUB + s.depth * SUB / 2);
        let cells = (cx, cy, cx + s.width, cy + s.depth);
        let (hx, hy) = (s.width * SUB / 2, s.depth * SUB / 2);
        // Ground units whose discs reach onto the footprint, the builder included.
        let on_site = |u: &super::Unit| {
            let t = &self.types[u.kind];
            let r = t.movement.radius;
            t.movement.altitude == 0
                && t.structure.is_none()
                && (u.pos.x - x).abs() < hx + r
                && (u.pos.y - y).abs() < hy + r
        };
        let me = &self.units[i];
        let reach = i64::from(self.types[me.kind].movement.radius + BUILD_REACH);
        let (dx, dy) = (i64::from(((me.pos.x - x).abs() - hx).max(0)), i64::from(((me.pos.y - y).abs() - hy).max(0)));
        let near = dx * dx + dy * dy <= reach * reach;
        if near && !on_site(me) {
            self.units[i].goal = None;
            if self.units.iter().any(on_site) {
                return; // Wait for the site to clear.
            }
            let owner = self.units[i].owner;
            let frame = self.place(owner, kind, x, y, true);
            self.reblock();
            let by = self.units[i].id;
            let u = &mut self.units[i];
            (u.plan, u.assist) = (None, Some(frame));
            events.push(Event::Placed { by, unit: frame });
        } else if self.units[i].goal.is_none() {
            if let Some((gx, gy)) = self.beside(cells, me.pos.x, me.pos.y) {
                self.set_goal(i, gx, gy);
            } else {
                self.units[i].plan = None;
            }
        }
    }
}
