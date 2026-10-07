//! The game state and its tick: units, the orders given to them, and movement over the terrain.
//!
//! Units follow their order's flow field from cell to cell. Within a tick a unit spends its speed budget along
//! the way, carrying what is left past each waypoint, and slows when the ground ahead climbs. Units do not yet
//! block or steer round each other; that is the next movement step.

use crate::movement::{FlowField, MoveClass};
use crate::space::{SUB, Vec3};
use crate::terrain::Heightmap;
use rts_core::hash::{Canon, CanonHasher};
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
}

impl Canon for Unit {
    fn canon(&self, w: &mut CanonHasher) {
        let goal = self.goal.map(|(x, y)| vec![x, y]);
        w.object()
            .field("class", &(self.class as u32))
            .opt("goal", goal.as_ref())
            .field("id", &self.id)
            .field("pos", &self.pos)
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
        self.units.push(Unit { id, class, pos: Vec3::new(x, y, z), goal: None });
        id
    }

    /// Queue a command for the next tick.
    pub fn command(&mut self, command: Command) {
        self.commands.push(self.tick, command);
    }

    /// Run one tick: apply this tick's commands, then move every unit in id order.
    pub fn step(&mut self) -> Vec<Event> {
        for command in self.commands.take(self.tick) {
            self.apply(command);
        }
        let mut events = Vec::new();
        for unit in &mut self.units {
            if let Some(goal) = unit.goal {
                let class = &self.classes[unit.class];
                let field = &self.fields[&(unit.class, cell_of(&self.map, goal))];
                if let Some(reason) = advance(&self.map, class, field, unit, goal) {
                    unit.goal = None;
                    events.push(Event::MoveEnded { unit: unit.id, reason, x: unit.pos.x, y: unit.pos.y });
                }
            }
        }
        self.tick += 1;
        events
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
            }
            Command::Stop { unit } => {
                if let Ok(i) = self.units.binary_search_by_key(&unit, |u| u.id) {
                    self.units[i].goal = None;
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
