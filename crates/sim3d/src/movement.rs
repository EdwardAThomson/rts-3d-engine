//! Movement over heightmap terrain: what a class of mover can climb, and flow fields that send every mover of a
//! class to one goal along the cheapest route.
//!
//! A flow field is built once per class and goal cell and shared by every unit given that order, so a group
//! costs one search rather than one per unit. The idea of flow fields for RTS groups is the well-known one from
//! the crowd-pathfinding literature (Emerson, "Crowd Pathfinding and Steering Using Flow Field Tiles", Game AI
//! Pro, 2013); the code here is our own.

use crate::space::SUB;
use crate::terrain::Heightmap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Ground length of one diagonal cell step: `SUB` times the square root of two, rounded to the nearest unit.
pub const DIAG: i32 = 362;

/// The eight neighbouring cells, clockwise from north (y grows southwards). This order also breaks ties, so
/// every machine picks the same route.
const NEIGHBOURS: [(i32, i32); 8] = [(0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1)];

/// How one class of mover deals with the ground. Classes come from data; the engine never assumes which ones
/// exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveClass {
    /// Sub-cell units covered per tick on level ground.
    pub speed: i32,
    /// The steepest slope this class can cross, up or down, in height units per cell of ground travelled.
    /// `None` for classes that ignore the shape of the ground, such as aircraft.
    pub max_slope: Option<i32>,
    /// Percent of speed lost when climbing at `max_slope`; gentler climbs lose proportionally less and going
    /// downhill costs nothing. Ignored when `max_slope` is `None`. Must be below 100.
    pub climb_slowdown: i32,
    /// Height kept above the ground: 0 for ground units, more for aircraft. Units on the ground and units in
    /// the air never push each other.
    pub altitude: i32,
    /// Units are discs of this radius, in sub-cell units, and keep apart from other units in their layer. At
    /// most half a cell.
    pub radius: i32,
}

impl MoveClass {
    /// Whether this class may travel `run` sub-cell units of ground while the ground changes height by `rise`.
    pub fn can_cross(&self, rise: i32, run: i32) -> bool {
        self.max_slope.is_none_or(|max| i64::from(rise).abs() * i64::from(SUB) <= i64::from(max) * i64::from(run))
    }

    /// Percent of full speed kept while travelling `run` sub-cell units of ground and climbing `rise` height
    /// units. Never below 1, so a unit on a crossable slope always makes progress.
    pub fn speed_percent(&self, rise: i32, run: i32) -> i32 {
        match self.max_slope {
            Some(max) if rise > 0 && run > 0 && max > 0 => {
                // Lost percent = slowdown * (rise per cell) / max, capped at the slowdown itself.
                let lost = i64::from(self.climb_slowdown) * i64::from(rise) * i64::from(SUB)
                    / (i64::from(max) * i64::from(run));
                (100 - lost.min(i64::from(self.climb_slowdown)) as i32).max(1)
            }
            _ => 100,
        }
    }
}

/// The cheapest way from every cell of the map to one goal cell for one class of mover. Costs are ground
/// distance stretched by climbing, so routes go round steep hills when that is quicker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowField {
    width: i32,
    goal: (i32, i32),
    /// Cost to reach the goal from each cell, in row order; `u32::MAX` where the goal cannot be reached.
    cost: Vec<u32>,
    /// The next cell on the way to the goal, as a row-order index; -1 at the goal and where it cannot be reached.
    next: Vec<i32>,
}

/// Ground height at the centre of every cell, in row order. Movers judge slopes between cell centres.
fn centre_heights(map: &Heightmap) -> Vec<i32> {
    let mut heights = Vec::with_capacity((map.width() * map.height()) as usize);
    for cy in 0..map.height() {
        for cx in 0..map.width() {
            heights.push(map.sample(cx * SUB + SUB / 2, cy * SUB + SUB / 2));
        }
    }
    heights
}

/// The cost of one step from cell `a` to the neighbouring cell `b`, or `None` if `class` cannot make it. A
/// diagonal step also needs both cells it squeezes between to be crossable from `a`, so units never cut a
/// corner of a cliff.
fn step_cost(
    size: (i32, i32),
    heights: &[i32],
    blocked: &[bool],
    class: &MoveClass,
    a: (i32, i32),
    b: (i32, i32),
) -> Option<u32> {
    step_cost_by(size, |(x, y)| heights[(y * size.0 + x) as usize], blocked, class, a, b)
}

/// `step_cost` with the cell-centre heights given by a function, for callers without a table of them.
fn step_cost_by(
    size: (i32, i32),
    h: impl Fn((i32, i32)) -> i32,
    blocked: &[bool],
    class: &MoveClass,
    a: (i32, i32),
    b: (i32, i32),
) -> Option<u32> {
    let (w, hgt) = size;
    let inside = |(x, y): (i32, i32)| (0..w).contains(&x) && (0..hgt).contains(&y);
    if !inside(a) || !inside(b) {
        return None;
    }
    // Structures block ground units, which also may not squeeze diagonally past one.
    let diagonal = a.0 != b.0 && a.1 != b.1;
    let solid = |(x, y): (i32, i32)| blocked.get((y * w + x) as usize).copied().unwrap_or(false);
    if class.altitude == 0 && (solid(a) || solid(b) || (diagonal && (solid((b.0, a.1)) || solid((a.0, b.1))))) {
        return None;
    }
    if diagonal {
        for side in [(b.0, a.1), (a.0, b.1)] {
            if !class.can_cross(h(side) - h(a), SUB) {
                return None;
            }
        }
    }
    let run = if diagonal { DIAG } else { SUB };
    let rise = h(b) - h(a);
    if !class.can_cross(rise, run) {
        return None;
    }
    Some((run * 100 / class.speed_percent(rise, run)) as u32)
}

/// Whether `class` may move from cell `a` into cell `b`, which is the same cell or one of its eight neighbours,
/// by the same rules the flow fields use. Cells marked in `blocked`, one per cell in row order, are closed to ground
/// classes; an empty slice blocks nothing.
pub fn can_step(map: &Heightmap, blocked: &[bool], class: &MoveClass, a: (i32, i32), b: (i32, i32)) -> bool {
    if a == b {
        return true;
    }
    let h = |(cx, cy): (i32, i32)| map.sample(cx * SUB + SUB / 2, cy * SUB + SUB / 2);
    (a.0 - b.0).abs() <= 1
        && (a.1 - b.1).abs() <= 1
        && step_cost_by((map.width(), map.height()), h, blocked, class, a, b).is_some()
}

impl FlowField {
    /// Search the whole map outwards from `goal` (a cell, clamped onto the map) with Dijkstra's algorithm. Cells
    /// marked in `blocked` are closed to ground classes, as in `can_step`.
    pub fn build(map: &Heightmap, blocked: &[bool], class: &MoveClass, goal: (i32, i32)) -> Self {
        let (w, hgt) = (map.width(), map.height());
        let goal = (goal.0.clamp(0, w - 1), goal.1.clamp(0, hgt - 1));
        let heights = centre_heights(map);
        let cells = (w * hgt) as usize;
        let index = |(x, y): (i32, i32)| (y * w + x) as usize;
        let cell = |i: usize| (i as i32 % w, i as i32 / w);

        let mut cost = vec![u32::MAX; cells];
        let mut open = BinaryHeap::new();
        cost[index(goal)] = 0;
        open.push(Reverse((0u32, index(goal))));
        while let Some(Reverse((c, i))) = open.pop() {
            if c > cost[i] {
                continue;
            }
            let b = cell(i);
            // Walk backwards: the cost that matters is moving from the neighbour `a` into `b`.
            for (dx, dy) in NEIGHBOURS {
                let a = (b.0 + dx, b.1 + dy);
                if let Some(step) = step_cost((w, hgt), &heights, blocked, class, a, b) {
                    let total = c + step;
                    if total < cost[index(a)] {
                        cost[index(a)] = total;
                        open.push(Reverse((total, index(a))));
                    }
                }
            }
        }

        let mut next = vec![-1; cells];
        for i in 0..cells {
            if cost[i] == u32::MAX || i == index(goal) {
                continue;
            }
            let a = cell(i);
            // Among equally cheap steps, take the one whose cell is nearest the goal in a straight line, so a
            // route and its mirror image make the same choices; any tie left goes to the neighbour order.
            let mut best: Option<(u32, i64, usize)> = None;
            for (dx, dy) in NEIGHBOURS {
                let b = (a.0 + dx, a.1 + dy);
                if let Some(step) = step_cost((w, hgt), &heights, blocked, class, a, b)
                    && cost[index(b)] != u32::MAX
                {
                    let total = step + cost[index(b)];
                    let (gx, gy) = (i64::from(goal.0 - b.0), i64::from(goal.1 - b.1));
                    let line = gx * gx + gy * gy;
                    if best.is_none_or(|(t, l, _)| (total, line) < (t, l)) {
                        best = Some((total, line, index(b)));
                    }
                }
            }
            next[i] = best.map_or(-1, |(_, _, j)| j as i32);
        }
        Self { width: w, goal, cost, next }
    }

    pub fn goal(&self) -> (i32, i32) {
        self.goal
    }

    /// The cost from a cell to the goal, or `None` if the goal cannot be reached from it.
    pub fn cost(&self, cell: (i32, i32)) -> Option<u32> {
        self.cost.get((cell.1 * self.width + cell.0) as usize).copied().filter(|&c| c != u32::MAX)
    }

    /// The neighbouring cell to head for next, or `None` at the goal or where the goal cannot be reached.
    pub fn next(&self, cell: (i32, i32)) -> Option<(i32, i32)> {
        let n = *self.next.get((cell.1 * self.width + cell.0) as usize)?;
        (n >= 0).then(|| (n % self.width, n / self.width))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rts_core::imath::isqrt;

    fn tracked() -> MoveClass {
        MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: 64 }
    }

    #[test]
    fn diag_is_root_two_cells() {
        let exact = isqrt(2 * (SUB as u64) * (SUB as u64) * 4) as i32; // twice the length, to round to nearest
        assert_eq!(DIAG, (exact + 1) / 2);
    }

    #[test]
    fn slope_limit_and_climb_slowdown() {
        let class = tracked();
        assert!(class.can_cross(64, SUB));
        assert!(class.can_cross(-64, SUB), "the limit holds downhill too");
        assert!(!class.can_cross(65, SUB));
        assert!(class.can_cross(90, DIAG), "a diagonal step spreads the same rise over more ground");
        assert_eq!(class.speed_percent(0, SUB), 100);
        assert_eq!(class.speed_percent(-64, SUB), 100, "going down costs nothing");
        assert_eq!(class.speed_percent(32, SUB), 75);
        assert_eq!(class.speed_percent(64, SUB), 50);
        let air = MoveClass { max_slope: None, ..class };
        assert!(air.can_cross(10_000, SUB));
        assert_eq!(air.speed_percent(10_000, SUB), 100);
    }

    #[test]
    fn flat_field_points_straight_at_the_goal() {
        let map = Heightmap::flat(5, 5, 0);
        let field = FlowField::build(&map, &[], &tracked(), (4, 4));
        assert_eq!(field.cost((4, 4)), Some(0));
        assert_eq!(field.next((4, 4)), None);
        assert_eq!(field.next((0, 0)), Some((1, 1)));
        assert_eq!(field.next((4, 0)), Some((4, 1)));
        assert_eq!(field.cost((0, 0)), Some(4 * DIAG as u32));
    }

    #[test]
    fn no_corner_cutting_past_a_cliff() {
        // Two low cells on one diagonal, two towers on the other.
        let towers = [0, 900, 900, 0];
        assert_eq!(step_cost((2, 2), &towers, &[], &tracked(), (0, 0), (1, 1)), None);
        assert_eq!(step_cost((2, 2), &[0; 4], &[], &tracked(), (0, 0), (1, 1)), Some(DIAG as u32));
        assert_eq!(step_cost((2, 2), &[0; 4], &[], &tracked(), (1, 1), (2, 2)), None, "off the map");
    }

    #[test]
    fn climbing_costs_more_than_descending() {
        let ramp = [0, 32];
        let up = step_cost((2, 1), &ramp, &[], &tracked(), (0, 0), (1, 0)).unwrap();
        let down = step_cost((2, 1), &ramp, &[], &tracked(), (1, 0), (0, 0)).unwrap();
        assert_eq!((up, down), (SUB as u32 * 100 / 75, SUB as u32));
    }
}
