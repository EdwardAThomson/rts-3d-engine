//! The game state and its tick: units, the orders given to them, and movement over the terrain.
//!
//! Units follow their order's flow field from cell to cell. Within a tick a unit spends its speed budget along
//! the way, carrying what is left past each waypoint, and slows when the ground ahead climbs.
//!
//! Units are discs and keep apart. After everyone has moved, every overlapping pair in a layer (ground or air)
//! is pushed apart: a moving unit shoves an idle one out of its way, and two moving units share the push along
//! an axis turned a little clockwise, so units meeting head on slide round each other instead of locking. A
//! push never takes a unit off the map or onto ground its class cannot cross. A group sent to one point packs
//! round it: a unit whose move is not over arrives when it touches a unit already resting at the same goal and
//! is within the space such a crowd needs.
//! The push-apart idea is the separation rule of Reynolds' boids ("Steering Behaviors For Autonomous
//! Characters", 1999); the code is our own.

use crate::movement::{FlowField, MoveClass, can_step};
use crate::space::{SUB, Vec3};
use crate::terrain::Heightmap;
use rts_core::hash::{Canon, CanonHasher};
use rts_core::imath::isqrt;
use rts_core::replay::{CommandQueue, Logged};
use std::collections::BTreeMap;

/// An order from a player or the AI. Commands are the only input to the simulation, so the starting state, the
/// seed and the command log replay a game exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Go to a point on the map, given in sub-cell units. Points off the map are moved onto its edge.
    Move { unit: u32, x: i32, y: i32 },
    /// Stop where it stands.
    Stop { unit: u32 },
}

/// Why a move ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEnd {
    Arrived,
    /// The unit's class cannot get from where it stands to the goal.
    Unreachable,
}

/// Something that happened in a tick, for rendering, audio and logs. Nothing here feeds back into the state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    MoveEnded { unit: u32, reason: MoveEnd, x: i32, y: i32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub id: u32,
    /// Index into the world's movement classes.
    pub class: usize,
    pub pos: Vec3,
    /// Where the unit is going, in sub-cell units.
    pub goal: Option<(i32, i32)>,
    /// The goal of the move this unit last finished by arriving, until it is given a new order. Units still
    /// heading there stop when they touch it.
    pub rest: Option<(i32, i32)>,
}

impl Canon for Unit {
    fn canon(&self, w: &mut CanonHasher) {
        let goal = self.goal.map(|(x, y)| vec![x, y]);
        let rest = self.rest.map(|(x, y)| vec![x, y]);
        w.object()
            .field("class", &(self.class as u32))
            .opt("goal", goal.as_ref())
            .field("id", &self.id)
            .field("pos", &self.pos)
            .opt("rest", rest.as_ref())
            .end();
    }
}

#[derive(Clone, Debug)]
pub struct World {
    map: Heightmap,
    classes: Vec<MoveClass>,
    /// In id order, which is the order they were spawned in.
    units: Vec<Unit>,
    next_id: u32,
    tick: u32,
    commands: CommandQueue<Command>,
    /// One flow field per class and goal cell, shared by every unit sent there. Derived from the map, so it is
    /// a cache and not part of the state hash.
    fields: BTreeMap<(usize, (i32, i32)), FlowField>,
}

impl World {
    /// A world on `map` whose units move in the given classes, normally read from a setting pack.
    pub fn new(map: Heightmap, classes: Vec<MoveClass>) -> Self {
        for class in &classes {
            assert!(class.speed > 0, "a movement class must move");
            assert!((0..100).contains(&class.climb_slowdown), "climb slowdown is a percent below 100");
            assert!((1..=SUB / 2).contains(&class.radius), "a unit's radius is from 1 to half a cell");
        }
        Self {
            map,
            classes,
            units: Vec::new(),
            next_id: 1,
            tick: 0,
            commands: CommandQueue::default(),
            fields: BTreeMap::new(),
        }
    }

    pub fn map(&self) -> &Heightmap {
        &self.map
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.units.binary_search_by_key(&id, |u| u.id).ok().map(|i| &self.units[i])
    }

    pub fn command_log(&self) -> &[Logged<Command>] {
        self.commands.log()
    }

    /// Place a new unit of `class` on the ground at `(x, y)` and return its id.
    pub fn spawn(&mut self, class: usize, x: i32, y: i32) -> u32 {
        let (x, y) = self.onto_map(x, y);
        let id = self.next_id;
        self.next_id += 1;
        let z = self.map.sample(x, y) + self.classes[class].altitude;
        self.units.push(Unit { id, class, pos: Vec3::new(x, y, z), goal: None, rest: None });
        id
    }

    /// Queue a command for the next tick.
    pub fn command(&mut self, command: Command) {
        self.commands.push(self.tick, command);
    }

    /// Run one tick: apply this tick's commands, move every unit in id order, push overlapping units apart,
    /// then end the moves of units that reached their goal or touched a unit resting there.
    pub fn step(&mut self) -> Vec<Event> {
        for command in self.commands.take(self.tick) {
            self.apply(command);
        }
        let mut ended = Vec::new();
        for (i, unit) in self.units.iter_mut().enumerate() {
            if let Some(goal) = unit.goal {
                let class = &self.classes[unit.class];
                let field = &self.fields[&(unit.class, cell_of(&self.map, goal))];
                if let Some(reason) = advance(&self.map, class, field, unit, goal) {
                    unit.goal = None;
                    unit.rest = (reason == MoveEnd::Arrived).then_some(goal);
                    ended.push((i, reason));
                }
            }
        }
        self.separate();
        self.arrive_by_contact(&mut ended);
        self.tick += 1;
        ended
            .into_iter()
            .map(|(i, reason)| {
                let u = &self.units[i];
                Event::MoveEnded { unit: u.id, reason, x: u.pos.x, y: u.pos.y }
            })
            .collect()
    }

    /// The indices of units by the cell they stand in, each list in id order.
    fn buckets(&self) -> Vec<Vec<usize>> {
        let mut buckets = vec![Vec::new(); (self.map.width() * self.map.height()) as usize];
        for (i, u) in self.units.iter().enumerate() {
            let (cx, cy) = cell_of(&self.map, (u.pos.x, u.pos.y));
            buckets[(cy * self.map.width() + cx) as usize].push(i);
        }
        buckets
    }

    /// Every pair of units `(i, j)`, `i < j`, in the same layer whose discs are closer than `slack` beyond
    /// touching, in a fixed order. Radii are at most half a cell, so only neighbouring cells need checking.
    fn close_pairs(&self, slack: i32) -> Vec<(usize, usize)> {
        let buckets = self.buckets();
        let (w, h) = (self.map.width(), self.map.height());
        let mut pairs = Vec::new();
        for (i, a) in self.units.iter().enumerate() {
            let ca = &self.classes[a.class];
            let (cx, cy) = cell_of(&self.map, (a.pos.x, a.pos.y));
            for ny in (cy - 1).max(0)..=(cy + 1).min(h - 1) {
                for nx in (cx - 1).max(0)..=(cx + 1).min(w - 1) {
                    for &j in &buckets[(ny * w + nx) as usize] {
                        let b = &self.units[j];
                        let cb = &self.classes[b.class];
                        if j <= i || (ca.altitude > 0) != (cb.altitude > 0) {
                            continue;
                        }
                        let reach = i64::from(ca.radius + cb.radius + slack);
                        let (dx, dy) = (i64::from(b.pos.x - a.pos.x), i64::from(b.pos.y - a.pos.y));
                        if dx * dx + dy * dy < reach * reach {
                            pairs.push((i, j));
                        }
                    }
                }
            }
        }
        pairs.sort_unstable();
        pairs
    }

    /// Push overlapping units apart. Pushes are summed first and applied together, so the result does not
    /// depend on which pair is looked at first.
    fn separate(&mut self) {
        let mut push = vec![(0i64, 0i64); self.units.len()];
        for (i, j) in self.close_pairs(0) {
            let (a, b) = (&self.units[i], &self.units[j]);
            let (mut dx, mut dy) = (i64::from(b.pos.x - a.pos.x), i64::from(b.pos.y - a.pos.y));
            let d = isqrt((dx * dx + dy * dy) as u64) as i64;
            let reach = i64::from(self.classes[a.class].radius + self.classes[b.class].radius);
            let overlap = reach - d;
            let mut dist = d;
            if d == 0 {
                // Exactly on top of each other: part them in a direction fixed by their ids.
                (dx, dy) = PART[((a.id + b.id) % 8) as usize];
                dist = isqrt((dx * dx + dy * dy) as u64) as i64;
            }
            let (a_moving, b_moving) = (a.goal.is_some(), b.goal.is_some());
            // How far each is pushed, from `a` towards `b`. Rounding up makes sure they come apart.
            let (share_a, share_b, turn) = match (a_moving, b_moving) {
                (true, false) => (0, overlap, true),
                (false, true) => (overlap, 0, true),
                (true, true) => ((overlap + 1) / 2, (overlap + 1) / 2, true),
                (false, false) => ((overlap + 1) / 2, (overlap + 1) / 2, false),
            };
            // The axis of the push, turned clockwise by about 27 degrees when either is moving, so idle units step
            // aside rather than being bulldozed along the mover's line.
            let (ax, ay) = if turn { (dx - dy / 2, dy + dx / 2) } else { (dx, dy) };
            let along = |share: i64| ((ax * share).div_euclid(dist), (ay * share).div_euclid(dist));
            let (pax, pay) = along(share_a);
            let (pbx, pby) = along(share_b);
            push[i].0 -= pax;
            push[i].1 -= pay;
            push[j].0 += pbx;
            push[j].1 += pby;
        }
        for (i, (px, py)) in push.into_iter().enumerate() {
            if (px, py) == (0, 0) {
                continue;
            }
            let u = &self.units[i];
            let class = &self.classes[u.class];
            let to = self.onto_map((i64::from(u.pos.x) + px) as i32, (i64::from(u.pos.y) + py) as i32);
            if can_step(&self.map, class, cell_of(&self.map, (u.pos.x, u.pos.y)), cell_of(&self.map, to)) {
                let z = self.map.sample(to.0, to.1) + class.altitude;
                self.units[i].pos = Vec3::new(to.0, to.1, z);
            }
        }
    }

    /// A moving unit touching a unit that rests at its goal, close enough to the goal for the size of the crowd
    /// already there, has got as close as it can: its move ends there. Farther out it keeps driving and shoves
    /// resting units aside, so a group packs round the goal instead of queueing in a tail behind it. Units are
    /// checked in a fixed order, and one that stops can stop the next.
    fn arrive_by_contact(&mut self, ended: &mut Vec<(usize, MoveEnd)>) {
        let pairs = self.close_pairs(CONTACT);
        let mut resting: BTreeMap<(i32, i32), i64> = BTreeMap::new();
        for u in &self.units {
            if let (None, Some(rest)) = (u.goal, u.rest) {
                *resting.entry(rest).or_default() += 1;
            }
        }
        let mut changed = true;
        while std::mem::take(&mut changed) {
            for &(i, j) in &pairs {
                for (m, r) in [(i, j), (j, i)] {
                    let Some(goal) = self.units[m].goal else { continue };
                    if self.units[r].goal.is_some() || self.units[r].rest != Some(goal) {
                        continue;
                    }
                    // A crowd of n discs of radius r fits well inside (2 * sqrt(n) + 1) * r of its centre.
                    let crowd = resting[&goal];
                    let reach =
                        i64::from(self.classes[self.units[m].class].radius) * (2 * isqrt(crowd as u64) as i64 + 1);
                    let (dx, dy) = (i64::from(self.units[m].pos.x - goal.0), i64::from(self.units[m].pos.y - goal.1));
                    if dx * dx + dy * dy <= reach * reach {
                        self.units[m].goal = None;
                        self.units[m].rest = Some(goal);
                        *resting.entry(goal).or_default() += 1;
                        ended.push((m, MoveEnd::Arrived));
                        changed = true;
                    }
                }
            }
        }
        ended.sort_unstable_by_key(|&(i, _)| i);
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Move { unit, x, y } => {
                let goal = self.onto_map(x, y);
                let Ok(i) = self.units.binary_search_by_key(&unit, |u| u.id) else { return };
                let class = self.units[i].class;
                let cell = cell_of(&self.map, goal);
                let (map, classes) = (&self.map, &self.classes);
                self.fields.entry((class, cell)).or_insert_with(|| FlowField::build(map, &classes[class], cell));
                self.units[i].goal = Some(goal);
                self.units[i].rest = None;
            }
            Command::Stop { unit } => {
                if let Ok(i) = self.units.binary_search_by_key(&unit, |u| u.id) {
                    self.units[i].goal = None;
                    self.units[i].rest = None;
                }
            }
        }
    }

    /// The nearest point on the map, in sub-cell units. The far edges belong to the next cell, so they are just
    /// outside.
    fn onto_map(&self, x: i32, y: i32) -> (i32, i32) {
        (x.clamp(0, self.map.width() * SUB - 1), y.clamp(0, self.map.height() * SUB - 1))
    }
}

impl Canon for World {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("map", &self.map)
            .field("nextId", &self.next_id)
            .field("tick", &self.tick)
            .array("units", &self.units)
            .end();
    }
}

/// How close, beyond touching, a moving unit must come to a resting one to stop beside it.
const CONTACT: i32 = 8;

/// Directions to part two units standing on exactly the same point, clockwise from north.
const PART: [(i64, i64); 8] = [(0, -16), (11, -11), (16, 0), (11, 11), (0, 16), (-11, 11), (-16, 0), (-11, -11)];

/// The cell holding a point on the map.
fn cell_of(map: &Heightmap, (x, y): (i32, i32)) -> (i32, i32) {
    ((x / SUB).clamp(0, map.width() - 1), (y / SUB).clamp(0, map.height() - 1))
}

fn centre((cx, cy): (i32, i32)) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Move one unit for one tick. Returns how its move ended, if it did.
fn advance(
    map: &Heightmap,
    class: &MoveClass,
    field: &FlowField,
    unit: &mut Unit,
    goal: (i32, i32),
) -> Option<MoveEnd> {
    let mut budget = class.speed;
    let mut first = true;
    let mut ended = None;
    // A unit passes at most a few waypoints a tick; the cap only guards against a speed above a cell a tick.
    for _ in 0..8 {
        let here = (unit.pos.x, unit.pos.y);
        if here == goal {
            ended = Some(MoveEnd::Arrived);
            break;
        }
        let cell = cell_of(map, here);
        let target = if cell == field.goal() {
            goal
        } else {
            match field.next(cell) {
                Some(next) => centre(next),
                None => {
                    ended = Some(MoveEnd::Unreachable);
                    break;
                }
            }
        };
        if budget == 0 {
            break;
        }
        let to = Vec3::new(target.0, target.1, 0);
        let d = unit.pos.ground_distance(to) as i32;
        if first {
            // Climbing slows the whole tick, judged over the ground the unit would cover at full speed.
            let run = budget.min(d);
            let ahead = unit.pos.lerp(to, run, d);
            let rise = map.sample(ahead.x, ahead.y) - map.sample(here.0, here.1);
            budget = (budget * class.speed_percent(rise, run) / 100).max(1);
            first = false;
        }
        let step = budget.min(d);
        let p = unit.pos.lerp(to, step, d);
        unit.pos = Vec3::new(p.x, p.y, 0);
        budget -= step;
        if step == d {
            unit.pos.x = target.0;
            unit.pos.y = target.1;
        }
    }
    unit.pos.z = map.sample(unit.pos.x, unit.pos.y) + class.altitude;
    ended
}
