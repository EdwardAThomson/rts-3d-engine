//! The game state and its tick: units, the orders given to them, combat, and movement over the terrain.
//!
//! A tick runs in a fixed order: commands; then combat (every armed unit with a target fires or closes in, in
//! id order; every projectile flies, in id order; damage is applied in the order it was dealt; the dead are
//! removed in id order); then movement; then construction (builders head for their sites and place frames, see
//! `construction`); then the economy (income, then factories and builders pay for and build their items, see
//! `economy`). Everyone fires before anyone is hurt, so two units that kill each other
//! on the same tick both get their shot, whichever has the lower id. This follows the combat tick in
//! plans/rts/rules-combat.md.
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
//!
//! Armed units fight on their own: idle ones, and ones on an attack-move, look for an enemy every few ticks. A
//! unit on an attack-move halts while it has such a target and drives on when the target is gone.
//!
//! The push-apart idea is the separation rule of Reynolds' boids ("Steering Behaviors For Autonomous
//! Characters", 1999); the code is our own.

use crate::economy::{Job, Production, Spot, Store, Structure, Wreck, affordable, paid, reclaim_time};
use crate::movement::{FlowField, MoveClass, can_step};
use crate::space::{SUB, Vec3};
use crate::terrain::Heightmap;
use crate::vision::{FogRules, Vision};
use crate::weapon::{Projectile, Weapon};
use rts_core::hash::{Canon, CanonHasher};
use rts_core::imath::isqrt;
use rts_core::replay::{CommandQueue, Logged};
use rts_core::rng::{random_int, seed_state};
use std::collections::BTreeMap;

mod construction;
mod intent;

/// An order from a player or the AI. Commands are the only input to the simulation, so the starting state, the
/// seed and the command log replay a game exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Go to a point on the map, given in sub-cell units. Points off the map are moved onto its edge.
    Move { unit: u32, x: i32, y: i32 },
    /// Stop where it stands, and stop attacking.
    Stop { unit: u32 },
    /// Attack another unit: close in until it is in range and in sight, then fire whenever reloaded, until it
    /// is destroyed or another order is given.
    Attack { unit: u32, target: u32 },
    /// Go to a point like `Move`, but stop to fight any enemy met on the way: an armed unit halts to fire at an
    /// enemy it can hurt in range and in sight, and drives on once that enemy is gone or out of reach.
    AttackMove { unit: u32, x: i32, y: i32 },
    /// Add a unit type to the end of a factory's queue; with `repeat` it is queued again each time it is built.
    /// Ignored unless the factory can build that type.
    Produce { unit: u32, kind: usize, repeat: bool },
    /// Empty a factory's queue. What was already paid towards the item in progress is lost.
    ClearQueue { unit: u32 },
    /// Build a structure of type `kind` whose footprint's north-west cell is `(cx, cy)`: go next to the site,
    /// place a frame there once no ground unit stands on it, and build it. Ignored unless the unit can build that
    /// type and the site is on the map, clear of other structures and flat enough.
    Build { unit: u32, kind: usize, cx: i32, cy: i32 },
    /// Help build a frame of the unit's own side, going next to it first.
    Assist { unit: u32, target: u32 },
    /// Reclaim a wreck for resources, going next to it first.
    Reclaim { unit: u32, wreck: u32 },
    /// Patrol between where the unit stands and a point: attack-move there, then back, and so on until another
    /// order is given (see `intent`).
    Patrol { unit: u32, x: i32, y: i32 },
    /// A standing rule for a factory: whenever its queue is empty and its owner has fewer than `count` units of
    /// type `kind`, counting those queued in any of the owner's factories, queue one more. A count of 0 drops
    /// the rule. Ignored unless the factory can build that type.
    Keep { unit: u32, kind: usize, count: u32 },
    /// A standing rule for a unit: when damage takes its health below `percent` of its maximum, it drops what
    /// it is doing and moves to the point. A percent of 0 drops the rule.
    FallBack { unit: u32, percent: i32, x: i32, y: i32 },
    /// A standing rule for a factory: every unit it finishes moves to the point, given in sub-cell units. `None`
    /// drops the rule. Ignored unless the unit is a factory of mobile units.
    Rally { unit: u32, point: Option<(i32, i32)> },
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
    MoveEnded {
        unit: u32,
        reason: MoveEnd,
        x: i32,
        y: i32,
    },
    /// A unit fired a projectile at a point.
    Fired {
        unit: u32,
        projectile: u32,
        from: Vec3,
        aim: Vec3,
    },
    /// A projectile struck a unit, or the ground when `unit` is `None`, and burst.
    Impact {
        projectile: u32,
        at: Vec3,
        unit: Option<u32>,
    },
    /// A unit lost health.
    Damaged {
        unit: u32,
        by: u32,
        damage: i32,
        health: i32,
    },
    /// A unit was destroyed and removed.
    Destroyed {
        unit: u32,
        by: u32,
        at: Vec3,
    },
    /// A unit was finished: built by a factory, or a frame completed by a builder.
    Built {
        by: u32,
        unit: u32,
    },
    /// A builder placed the frame of a structure.
    Placed {
        by: u32,
        unit: u32,
    },
    /// A destroyed unit left a wreck.
    Wrecked {
        unit: u32,
        wreck: u32,
    },
    /// A builder finished reclaiming a wreck, which is gone.
    Reclaimed {
        by: u32,
        wreck: u32,
    },
}

/// A kind of unit, read from data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitType {
    pub movement: MoveClass,
    pub max_health: i32,
    /// Height of the unit's body above the point it stands on, in height units. Shots strike anywhere in the
    /// cylinder of this height and the movement radius, leave from its top and aim at its middle.
    pub height: i32,
    /// How far it sees, in cells, when the game has fog of war (`vision`); 0 sees nothing. Separate from its
    /// weapon's range, as in the Classic engine, so long guns need spotters.
    pub vision: i32,
    pub weapon: Option<Weapon>,
    /// Armour class, an index into every weapon's `against` list.
    pub armour: usize,
    /// What it costs, builds and produces.
    pub production: Production,
    /// Set for structures: units that never move and block ground movement.
    pub structure: Option<Structure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub id: u32,
    /// Index into the world's unit types.
    pub kind: usize,
    pub pos: Vec3,
    /// Where the unit is going, in sub-cell units.
    pub goal: Option<(i32, i32)>,
    /// The goal of the move this unit last finished by arriving, until it is given a new order. Units still
    /// heading there stop when they touch it.
    pub rest: Option<(i32, i32)>,
    pub health: i32,
    /// The player it belongs to. Units attack the units of any other owner they see on their own.
    pub owner: u8,
    /// The unit it is attacking.
    pub target: Option<u32>,
    /// Whether it was ordered to attack, and so closes in, or picked the target itself and only fires while it
    /// can.
    pub chase: bool,
    /// Ticks until its weapon has reloaded.
    pub cooldown: i32,
    /// Whether its current move is an attack-move, so it stops to fight enemies met on the way.
    pub hunt: bool,
    /// What it is producing, first item first.
    pub queue: Vec<Job>,
    /// Build work done on the first item in the queue.
    pub work: i64,
    /// Set while the unit is a frame still being built: the build work done on it so far. A frame does nothing
    /// but stand there and take damage.
    pub build: Option<i64>,
    /// The frame this builder is building.
    pub assist: Option<u32>,
    /// A structure this builder is on its way to place: its type and its footprint's north-west cell.
    pub plan: Option<(usize, i32, i32)>,
    /// The wreck this builder is reclaiming.
    pub reclaim: Option<u32>,
    /// On patrol: the other end of the route, where it heads once it reaches its goal.
    pub patrol: Option<(i32, i32)>,
    /// A factory's standing targets: unit types and how many of each its owner wants.
    pub keep: Vec<(usize, u32)>,
    /// Where to fall back to, and below what percent of its health.
    pub fall_back: Option<(i32, i32, i32)>,
    /// A factory's rally point, where every unit it finishes heads.
    pub rally: Option<(i32, i32)>,
}

impl Canon for Unit {
    fn canon(&self, w: &mut CanonHasher) {
        let goal = self.goal.map(|(x, y)| vec![x, y]);
        let rest = self.rest.map(|(x, y)| vec![x, y]);
        let plan = self.plan.map(|(k, x, y)| vec![k as i32, x, y]);
        let patrol = self.patrol.map(|(x, y)| vec![x, y]);
        let keep: Vec<Vec<i64>> = self.keep.iter().map(|&(k, n)| vec![k as i64, i64::from(n)]).collect();
        let fall_back = self.fall_back.map(|(p, x, y)| vec![p, x, y]);
        let rally = self.rally.map(|(x, y)| vec![x, y]);
        w.object()
            .opt("assist", self.assist.as_ref())
            .opt("build", self.build.as_ref())
            .field("chase", &self.chase)
            .field("cooldown", &self.cooldown)
            .opt("fallBack", fall_back.as_ref())
            .opt("goal", goal.as_ref())
            .field("health", &self.health)
            .opt("hunt", self.hunt.then_some(&true))
            .field("id", &self.id)
            .opt("keep", (!keep.is_empty()).then_some(&keep))
            .field("kind", &(self.kind as u32))
            .field("owner", &self.owner)
            .opt("patrol", patrol.as_ref())
            .opt("plan", plan.as_ref())
            .field("pos", &self.pos)
            .opt("queue", (!self.queue.is_empty()).then_some(&self.queue))
            .opt("rally", rally.as_ref())
            .opt("reclaim", self.reclaim.as_ref())
            .opt("rest", rest.as_ref())
            .opt("target", self.target.as_ref())
            .opt("work", (self.work != 0).then_some(&self.work))
            .end();
    }
}

#[derive(Clone, Debug)]
pub struct World {
    map: Heightmap,
    types: Vec<UnitType>,
    /// In id order, which is the order they were spawned in.
    units: Vec<Unit>,
    /// In id order. Projectiles take ids from the same counter as units.
    projectiles: Vec<Projectile>,
    next_id: u32,
    /// Each player's resources, in owner order. A player with no store has nothing to spend.
    stores: Vec<Store>,
    /// Resource spots on the map, in the order they were added.
    spots: Vec<Spot>,
    /// Wrecks, in id order.
    wrecks: Vec<Wreck>,
    /// The game's one random generator (scatter).
    rng: i32,
    tick: u32,
    commands: CommandQueue<Command>,
    /// One flow field per unit type and goal cell, shared by every unit sent there. Derived from the map, so it is
    /// a cache and not part of the state hash.
    fields: BTreeMap<(usize, (i32, i32)), FlowField>,
    /// The cells under a structure's footprint, one per cell in row order. Derived from the units, so it is a
    /// cache and not part of the state hash.
    blocked: Vec<bool>,
    /// Fog of war, when the game has it.
    vision: Option<Vision>,
}

impl World {
    /// A world on `map` with the given unit types, normally read from a setting pack, and a random seed.
    pub fn new(map: Heightmap, types: Vec<UnitType>, seed: i32) -> Self {
        for t in &types {
            let class = &t.movement;
            assert!(class.speed > 0 || t.structure.is_some(), "a movement class must move");
            if let Some(s) = t.structure {
                assert!(s.width > 0 && s.depth > 0 && s.max_rise >= 0, "a structure covers at least a cell");
            }
            assert!((0..100).contains(&class.climb_slowdown), "climb slowdown is a percent below 100");
            assert!((1..=SUB / 2).contains(&class.radius), "a unit's radius is from 1 to half a cell");
            assert!(t.max_health > 0 && t.height >= 0, "a unit has health and a height");
            let p = &t.production;
            assert!(p.build_power >= 0 && p.build_time >= 0, "no negative build values");
            assert!(p.cost.iter().chain(&p.produces).all(|&v| v >= 0), "no negative costs or income");
            for &kind in &p.builds {
                assert!(kind < types.len() && types[kind].production.build_time > 0, "a buildable type takes time");
            }
            if let Some(w) = &t.weapon {
                assert!(w.range > 0 && w.reload > 0 && w.speed > 0, "a weapon has range, reload and speed");
                assert!(
                    w.gravity >= 0 && w.damage >= 0 && w.splash >= 0 && w.scatter >= 0,
                    "no negative weapon values"
                );
            }
        }
        Self {
            map,
            types,
            units: Vec::new(),
            projectiles: Vec::new(),
            next_id: 1,
            stores: Vec::new(),
            spots: Vec::new(),
            wrecks: Vec::new(),
            rng: seed_state(seed),
            tick: 0,
            commands: CommandQueue::default(),
            fields: BTreeMap::new(),
            blocked: Vec::new(),
            vision: None,
        }
    }

    /// Turn fog of war on with these rules, or off. Part of setting up a game, before the first tick; a world
    /// starts without it.
    pub fn set_fog(&mut self, rules: Option<FogRules>) {
        self.vision = rules.map(|r| Vision::new(&self.map, r));
        self.update_vision();
    }

    /// The fog of war, if the game has it.
    pub fn vision(&self) -> Option<&Vision> {
        self.vision.as_ref()
    }

    /// Whether `player` sees unit `id` now: always without fog of war; with it, its own units and those in its sight.
    pub fn sees(&self, player: u8, id: u32) -> bool {
        self.unit(id).is_some_and(|u| self.sees_unit(player, u))
    }

    fn sees_unit(&self, player: u8, u: &Unit) -> bool {
        self.vision.as_ref().is_none_or(|v| v.sees(player, &self.types[u.kind], u))
    }

    fn update_vision(&mut self) {
        if let Some(v) = &mut self.vision {
            v.update(&self.map, &self.types, &self.units, self.tick);
        }
    }

    pub fn map(&self) -> &Heightmap {
        &self.map
    }

    /// The unit types, by kind.
    pub fn types(&self) -> &[UnitType] {
        &self.types
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    pub fn projectiles(&self) -> &[Projectile] {
        &self.projectiles
    }

    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.units.binary_search_by_key(&id, |u| u.id).ok().map(|i| &self.units[i])
    }

    /// A player's resources, if they have a store.
    pub fn store(&self, owner: u8) -> Option<&Store> {
        self.stores.iter().find(|s| s.owner == owner)
    }

    /// Give a player a store holding `amount` of each resource, up to `capacity`. Part of setting up a game,
    /// before the first tick.
    pub fn set_store(&mut self, owner: u8, amount: Vec<i64>, capacity: Vec<i64>) {
        assert!(amount.len() == capacity.len(), "one amount and one capacity per resource");
        let amount = amount.iter().zip(&capacity).map(|(&a, &c)| a.min(c)).collect();
        let store = Store { owner, amount, capacity, rate: 100 };
        match self.stores.binary_search_by_key(&owner, |s| s.owner) {
            Ok(i) => self.stores[i] = store,
            Err(i) => self.stores.insert(i, store),
        }
    }

    /// Add a resource spot at cell `(cx, cy)`. Part of making the map, before the first tick.
    pub fn add_spot(&mut self, cx: i32, cy: i32, resource: usize, rate: i64) {
        assert!(rate >= 0, "a spot yields, never costs");
        self.spots.push(Spot { cx, cy, resource, rate });
    }

    pub fn spots(&self) -> &[Spot] {
        &self.spots
    }

    pub fn wrecks(&self) -> &[Wreck] {
        &self.wrecks
    }

    pub fn command_log(&self) -> &[Logged<Command>] {
        self.commands.log()
    }

    /// Place a new unit of type `kind`, owned by player 0, on the ground at `(x, y)` and return its id.
    pub fn spawn(&mut self, kind: usize, x: i32, y: i32) -> u32 {
        self.spawn_for(0, kind, x, y)
    }

    /// Place a new unit of type `kind`, owned by `owner`, on the ground at `(x, y)` and return its id. A structure
    /// is moved to the nearest spot where its footprint lines up with the cells.
    pub fn spawn_for(&mut self, owner: u8, kind: usize, x: i32, y: i32) -> u32 {
        let id = self.place(owner, kind, x, y, false);
        if self.types[kind].structure.is_some() {
            self.reblock();
        }
        self.update_vision();
        id
    }

    /// Add a unit, finished or as a frame, without updating the blocked cells.
    fn place(&mut self, owner: u8, kind: usize, x: i32, y: i32, frame: bool) -> u32 {
        let (x, y) = match self.types[kind].structure {
            Some(s) => {
                let snap = |v: i32, cells: i32| (v - cells * SUB / 2 + SUB / 2).div_euclid(SUB) * SUB + cells * SUB / 2;
                (snap(x, s.width), snap(y, s.depth))
            }
            None => self.onto_map(x, y),
        };
        let id = self.next_id;
        self.next_id += 1;
        let t = &self.types[kind];
        let z = self.map.sample(x, y) + t.movement.altitude;
        let health = if frame { 1 } else { t.max_health };
        self.units.push(Unit {
            id,
            kind,
            pos: Vec3::new(x, y, z),
            goal: None,
            rest: None,
            health,
            owner,
            target: None,
            chase: false,
            cooldown: 0,
            hunt: false,
            queue: Vec::new(),
            work: 0,
            build: frame.then_some(0),
            assist: None,
            plan: None,
            reclaim: None,
            patrol: None,
            keep: Vec::new(),
            fall_back: None,
            rally: None,
        });
        id
    }

    /// Queue a command for the next tick.
    pub fn command(&mut self, command: Command) {
        self.commands.push(self.tick, command);
    }

    /// Run one tick: apply this tick's commands, run combat, move every unit in id order, push overlapping
    /// units apart, then end the moves of units that reached their goal or touched a unit resting there.
    pub fn step(&mut self) -> Vec<Event> {
        for command in self.commands.take(self.tick) {
            self.apply(command);
        }
        let mut events = Vec::new();
        let mut damage = Vec::new();
        self.fire(&mut events);
        self.fly(&mut events, &mut damage);
        let hurt = self.hurt(&mut events, damage);
        self.fall_back(hurt);
        let mut ended = Vec::new();
        for (i, unit) in self.units.iter_mut().enumerate() {
            if let Some(goal) = unit.goal {
                if unit.hunt && unit.target.is_some() {
                    // Halted on an attack-move to fight an enemy in range and in sight.
                    continue;
                }
                let class = &self.types[unit.kind].movement;
                let field = &self.fields[&(unit.kind, cell_of(&self.map, goal))];
                if let Some(reason) = advance(&self.map, class, field, unit, goal) {
                    (unit.goal, unit.hunt) = (None, false);
                    unit.rest = (reason == MoveEnd::Arrived).then_some(goal);
                    if reason == MoveEnd::Unreachable {
                        // A builder that cannot get to its site gives up on it.
                        (unit.plan, unit.assist, unit.reclaim) = (None, None, None);
                        unit.patrol = None;
                    }
                    ended.push((i, reason));
                }
            }
        }
        self.separate();
        self.arrive_by_contact(&mut ended);
        self.turn_patrols(&ended);
        events.extend(ended.into_iter().map(|(i, reason)| {
            let u = &self.units[i];
            Event::MoveEnded { unit: u.id, reason, x: u.pos.x, y: u.pos.y }
        }));
        self.construct(&mut events);
        self.economy(&mut events);
        self.tick += 1;
        self.update_vision();
        events
    }

    fn index_of(&self, id: u32) -> Option<usize> {
        self.units.binary_search_by_key(&id, |u| u.id).ok()
    }

    /// Send unit `i` towards a point, sharing the flow field for its type and the goal cell.
    fn set_goal(&mut self, i: usize, x: i32, y: i32) {
        let kind = self.units[i].kind;
        if self.types[kind].structure.is_some() {
            return;
        }
        let goal = self.onto_map(x, y);
        let cell = cell_of(&self.map, goal);
        let (map, blocked, types) = (&self.map, &self.blocked, &self.types);
        self.fields.entry((kind, cell)).or_insert_with(|| FlowField::build(map, blocked, &types[kind].movement, cell));
        self.units[i].goal = Some(goal);
        self.units[i].rest = None;
    }

    /// Every unit attacking something, in id order, reloads, then fires if it can or closes in if it cannot.
    fn fire(&mut self, events: &mut Vec<Event>) {
        for i in 0..self.units.len() {
            let unit = &mut self.units[i];
            unit.cooldown = (unit.cooldown - 1).max(0);
            let weapon = self.types[unit.kind].weapon.clone().filter(|_| unit.build.is_none());
            let Some(weapon) = weapon else {
                unit.target = None;
                continue;
            };
            if unit.target.is_none()
                && (unit.goal.is_none() || unit.hunt)
                && (self.tick + unit.id).is_multiple_of(SCAN_EVERY)
            {
                let found = self.acquire(i, &weapon);
                let unit = &mut self.units[i];
                (unit.target, unit.chase) = (found, false);
            }
            let unit = &self.units[i];
            let Some(target) = unit.target else { continue };
            let Some(t) = self.index_of(target) else {
                // The target is gone: the attack is over, and so is any chase.
                let unit = &mut self.units[i];
                if unit.chase {
                    unit.goal = None;
                }
                (unit.target, unit.chase) = (None, false);
                continue;
            };
            if !self.sees_unit(self.units[i].owner, &self.units[t]) {
                // Lost from sight: the attack ends, though a chase drives on to where it was last aimed.
                let unit = &mut self.units[i];
                (unit.target, unit.chase) = (None, false);
                continue;
            }
            let (muzzle, middle, in_range, in_sight) = self.aim(i, t, &weapon);
            let me = &self.units[i];
            if !(in_range && in_sight) && !me.chase {
                // A target picked by the unit itself is dropped once it slips out of range or sight; only an
                // ordered attack gives chase.
                self.units[i].target = None;
                continue;
            }
            if !(in_range && in_sight) {
                // Close in, re-aiming the chase whenever the target moves to another cell. If the target cannot be
                // reached, hold and keep watching rather than ordering a hopeless move every tick.
                let (x, y) = self.towards(i, t);
                let target_cell = cell_of(&self.map, (x, y));
                if me.goal.map(|g| cell_of(&self.map, g)) != Some(target_cell) {
                    let here = cell_of(&self.map, (me.pos.x, me.pos.y));
                    let kind = me.kind;
                    let (map, blocked, types) = (&self.map, &self.blocked, &self.types);
                    let field = self
                        .fields
                        .entry((kind, target_cell))
                        .or_insert_with(|| FlowField::build(map, blocked, &types[kind].movement, target_cell));
                    if field.cost(here).is_some() {
                        self.set_goal(i, x, y);
                    } else {
                        self.units[i].goal = None;
                    }
                }
                continue;
            }
            let me = &mut self.units[i];
            if !me.hunt {
                me.goal = None;
                me.rest = None;
            }
            if me.cooldown > 0 {
                continue;
            }
            me.cooldown = weapon.reload;
            let (shooter, owner, kind) = (me.id, me.owner, me.kind);
            let aim = self.scatter(middle, weapon.scatter);
            let id = self.next_id;
            self.next_id += 1;
            self.projectiles.push(Projectile::launch(id, (shooter, owner), kind, &weapon, muzzle, aim));
            events.push(Event::Fired { unit: shooter, projectile: id, from: muzzle, aim });
            // Firing gives the shooter away to the side it fired at.
            let victim = self.units[t].owner;
            if let Some(v) = &mut self.vision
                && victim != owner
            {
                let at = &self.units[i].pos;
                let until = self.tick + v.rules.reveal_ticks;
                v.reveal(victim, (at.x.div_euclid(SUB), at.y.div_euclid(SUB)), until);
            }
        }
    }

    /// Where unit `i` would fire from and aim at to hit unit `t`, and whether the target is in range and in
    /// sight. Direct fire needs a clear line to the target's middle, judged by flying the shot through the
    /// terrain exactly as it will fly; lobbed shots are fired regardless and may hit a hill.
    fn aim(&self, i: usize, t: usize, weapon: &Weapon) -> (Vec3, Vec3, bool, bool) {
        let (me, them) = (&self.units[i], &self.units[t]);
        let muzzle = Vec3::new(me.pos.x, me.pos.y, me.pos.z + self.types[me.kind].height);
        let middle = Vec3::new(them.pos.x, them.pos.y, them.pos.z + self.types[them.kind].height / 2);
        let in_range = i64::from(muzzle.ground_distance(middle)) <= i64::from(weapon.range);
        let in_sight = in_range
            && (weapon.gravity > 0
                || self.clear_shot(&Projectile::launch(0, (me.id, me.owner), me.kind, weapon, muzzle, middle)));
        (muzzle, middle, in_range, in_sight)
    }

    /// The enemy an idle unit picks for itself: the nearest unit of another owner that its weapon can hurt and
    /// that is in range and, for direct fire, in sight, ties going to the lower id. Only the nearest few in range are tried for sight.
    fn acquire(&self, i: usize, weapon: &Weapon) -> Option<u32> {
        let me = &self.units[i];
        let reach = i64::from(weapon.range);
        let mut near: Vec<(i64, u32, usize)> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.owner != me.owner && weapon.percent_against(self.types[u.kind].armour) > 0)
            .filter(|(_, u)| self.sees_unit(me.owner, u))
            .filter_map(|(t, u)| {
                let (dx, dy) = (i64::from(u.pos.x - me.pos.x), i64::from(u.pos.y - me.pos.y));
                let d2 = dx * dx + dy * dy;
                (d2 <= reach * reach).then_some((d2, u.id, t))
            })
            .collect();
        near.sort_unstable();
        near.into_iter().take(SIGHT_TRIES).find(|&(_, _, t)| self.aim(i, t, weapon).3).map(|(_, id, _)| id)
    }

    /// The aim point moved by a random offset within `radius` on the ground. Offsets are drawn in a square and
    /// redrawn until one lies in the circle, up to four draws, then no offset.
    fn scatter(&mut self, aim: Vec3, radius: i32) -> Vec3 {
        if radius == 0 {
            return aim;
        }
        let span = (2 * radius + 1) as u32;
        for _ in 0..4 {
            let dx = random_int(&mut self.rng, span) as i32 - radius;
            let dy = random_int(&mut self.rng, span) as i32 - radius;
            if i64::from(dx) * i64::from(dx) + i64::from(dy) * i64::from(dy) <= i64::from(radius) * i64::from(radius) {
                return Vec3::new(aim.x + dx, aim.y + dy, aim.z);
            }
        }
        aim
    }

    /// Fly every projectile one tick, in id order. Each is checked along its path, a few times a cell, against
    /// the ground and against every unit but its firer; the first thing it meets stops it. One that leaves the
    /// map, or flies on four times its planned flight without meeting anything, is gone.
    fn fly(&mut self, events: &mut Vec<Event>, damage: &mut Vec<Hurt>) {
        let mut flying = std::mem::take(&mut self.projectiles);
        flying.retain_mut(|p| {
            let before = p.at(p.flown);
            p.flown += 1;
            match self.contact(p.firer, before, p.at(p.flown), true) {
                Contact::Clear => p.flown < 4 * p.flight,
                Contact::OffMap => false,
                Contact::Struck(at, unit) => {
                    events.push(Event::Impact { projectile: p.id, at, unit });
                    self.blast(p, at, unit, damage);
                    false
                }
            }
        });
        self.projectiles = flying;
    }

    /// What a projectile meets first on its way from `before` to `after`, checked every `CHECK_EVERY` sub-cell
    /// units: the edge of the map, a unit other than `firer` (only when `units` is set), or the ground.
    fn contact(&self, firer: u32, before: Vec3, after: Vec3, units: bool) -> Contact {
        let steps = (before.ground_distance(after) as i32 / CHECK_EVERY + 1).max(1);
        for k in 1..=steps {
            let at = before.lerp(after, k, steps);
            if self.onto_map(at.x, at.y) != (at.x, at.y) {
                return Contact::OffMap;
            }
            if units
                && let Some(u) = self.units.iter().find(|u| {
                    let t = &self.types[u.kind];
                    u.id != firer && self.gap(u, at.x, at.y) == 0 && (u.pos.z..=u.pos.z + t.height).contains(&at.z)
                })
            {
                return Contact::Struck(at, Some(u.id));
            }
            let ground = self.map.sample(at.x, at.y);
            if at.z <= ground {
                return Contact::Struck(Vec3::new(at.x, at.y, ground), None);
            }
        }
        Contact::Clear
    }

    /// Whether a shot's whole planned flight stays clear of the ground, checked exactly as its flight will be.
    fn clear_shot(&self, p: &Projectile) -> bool {
        (0..p.flight).all(|t| matches!(self.contact(p.firer, p.at(t), p.at(t + 1), false), Contact::Clear))
    }

    /// The damage a projectile bursting at `at` deals: full to a unit it struck, then full to units whose
    /// centre is within half its splash and half to those within all of it, never to its firer. The firer's own
    /// side takes `FRIENDLY_SPLASH_PERCENT` of splash, so supporting fire is risky but not ruinous. Splash reaches
    /// units whose middle is within the splash distance above or below the burst, so a shell bursting on the
    /// ground does not hurt aircraft overhead.
    fn blast(&self, p: &Projectile, at: Vec3, struck: Option<u32>, damage: &mut Vec<Hurt>) {
        let Some(weapon) = &self.types[p.kind].weapon else { return };
        if let Some(id) = struck
            && let Some(t) = self.index_of(id)
        {
            let amount = weapon.damage_to(self.types[self.units[t].kind].armour, 100, 100);
            damage.push(Hurt { unit: id, by: p.firer, amount });
        }
        let splash = i64::from(weapon.splash);
        if splash == 0 {
            return;
        }
        for u in &self.units {
            if u.id == p.firer || Some(u.id) == struck {
                continue;
            }
            let middle = u.pos.z + self.types[u.kind].height / 2;
            let (dx, dy) = (i64::from(u.pos.x - at.x), i64::from(u.pos.y - at.y));
            let d2 = dx * dx + dy * dy;
            if i64::from(middle - at.z).abs() > splash || d2 > splash * splash {
                continue;
            }
            let band = if 4 * d2 <= splash * splash { 100 } else { 50 };
            let side = if u.owner == p.owner { FRIENDLY_SPLASH_PERCENT } else { 100 };
            let amount = weapon.damage_to(self.types[u.kind].armour, band, side);
            damage.push(Hurt { unit: u.id, by: p.firer, amount });
        }
    }

    /// Apply damage in the order it was dealt, then remove the destroyed in id order. Returns the survivors
    /// that were hurt, in id order, with their health before this tick's damage.
    fn hurt(&mut self, events: &mut Vec<Event>, damage: Vec<Hurt>) -> Vec<(u32, i32)> {
        let mut killer = BTreeMap::new();
        let mut before: BTreeMap<u32, i32> = BTreeMap::new();
        for h in damage {
            let Some(i) = self.index_of(h.unit) else { continue };
            let u = &mut self.units[i];
            if u.health <= 0 || h.amount == 0 {
                continue;
            }
            before.entry(u.id).or_insert(u.health);
            u.health -= h.amount;
            events.push(Event::Damaged { unit: u.id, by: h.by, damage: h.amount, health: u.health });
            if u.health <= 0 {
                killer.insert(u.id, h.by);
            }
        }
        for u in &self.units {
            if let Some(&by) = killer.get(&u.id) {
                events.push(Event::Destroyed { unit: u.id, by, at: u.pos });
                // A finished unit worth something leaves a wreck; a frame leaves nothing.
                if u.build.is_none() && !self.types[u.kind].production.wreck.is_empty() {
                    let id = self.next_id;
                    self.next_id += 1;
                    let pos = Vec3::new(u.pos.x, u.pos.y, self.map.sample(u.pos.x, u.pos.y));
                    self.wrecks.push(Wreck { id, kind: u.kind, pos, work: 0 });
                    events.push(Event::Wrecked { unit: u.id, wreck: id });
                }
            }
        }
        let types = &self.types;
        let fell = self.units.iter().any(|u| u.health <= 0 && types[u.kind].structure.is_some());
        self.units.retain(|u| u.health > 0);
        if fell {
            self.reblock();
        }
        before.into_iter().filter(|(id, _)| !killer.contains_key(id)).collect()
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
            let ca = &self.types[a.kind].movement;
            let (cx, cy) = cell_of(&self.map, (a.pos.x, a.pos.y));
            for ny in (cy - 1).max(0)..=(cy + 1).min(h - 1) {
                for nx in (cx - 1).max(0)..=(cx + 1).min(w - 1) {
                    for &j in &buckets[(ny * w + nx) as usize] {
                        let b = &self.units[j];
                        let cb = &self.types[b.kind].movement;
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
            if self.types[a.kind].structure.is_some() || self.types[b.kind].structure.is_some() {
                continue;
            }
            let (mut dx, mut dy) = (i64::from(b.pos.x - a.pos.x), i64::from(b.pos.y - a.pos.y));
            let d = isqrt((dx * dx + dy * dy) as u64) as i64;
            let reach = i64::from(self.types[a.kind].movement.radius + self.types[b.kind].movement.radius);
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
            let along = |share: i64| (ax * share / dist, ay * share / dist);
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
            let class = &self.types[u.kind].movement;
            let to = self.onto_map((i64::from(u.pos.x) + px) as i32, (i64::from(u.pos.y) + py) as i32);
            if can_step(&self.map, &self.blocked, class, cell_of(&self.map, (u.pos.x, u.pos.y)), cell_of(&self.map, to))
            {
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
                    let reach = i64::from(self.types[self.units[m].kind].movement.radius)
                        * (2 * isqrt(crowd as u64) as i64 + 1);
                    let (dx, dy) = (i64::from(self.units[m].pos.x - goal.0), i64::from(self.units[m].pos.y - goal.1));
                    if dx * dx + dy * dy <= reach * reach {
                        (self.units[m].goal, self.units[m].hunt) = (None, false);
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

    /// Income, then building. Every finished unit adds what it produces to its owner's store, and the store is
    /// capped. Then every spender (a factory working on its first queued item, or a builder next to the frame it
    /// is building) asks for what a full tick of its build power would cost. Where a player's store cannot cover
    /// all their requests for some resource, all of their spenders work at the same lower rate: the lowest share
    /// of what was asked that the store can pay, over all resources. Spenders are then paid in id order, each
    /// never taking more than is left. Finished units appear beside their factory; finished frames become whole.
    fn economy(&mut self, events: &mut Vec<Event>) {
        self.keep_stocked();
        for u in self.units.iter().filter(|u| u.build.is_none()) {
            let p = &self.types[u.kind].production;
            let Ok(s) = self.stores.binary_search_by_key(&u.owner, |s| s.owner) else { continue };
            let mut income = p.produces.clone();
            if p.extracts
                && let Some((x0, y0, x1, y1)) = self.footprint_cells(u.kind, u.pos.x, u.pos.y)
            {
                for spot in &self.spots {
                    if (x0..x1).contains(&spot.cx) && (y0..y1).contains(&spot.cy) {
                        if income.len() <= spot.resource {
                            income.resize(spot.resource + 1, 0);
                        }
                        income[spot.resource] += spot.rate;
                    }
                }
            }
            for (have, add) in self.stores[s].amount.iter_mut().zip(income) {
                *have += add;
            }
        }
        self.reclaim(events);
        let mut asked = vec![Vec::new(); self.stores.len()];
        for (s, store) in self.stores.iter_mut().enumerate() {
            for (amount, &cap) in store.amount.iter_mut().zip(&store.capacity) {
                *amount = (*amount).min(cap);
            }
            asked[s] = vec![0i64; store.amount.len()];
        }
        let spenders: Vec<Spender> = (0..self.units.len()).filter_map(|i| self.spender(i)).collect();
        for sp in &spenders {
            let Ok(s) = self.stores.binary_search_by_key(&self.units[sp.unit].owner, |s| s.owner) else { continue };
            let item = &self.types[sp.item].production;
            let from = self.work_of(sp.on);
            let to = (from + sp.power).min(item.build_time);
            for (r, ask) in asked[s].iter_mut().enumerate() {
                let cost = item.cost_of(r);
                *ask += paid(cost, to, item.build_time) - paid(cost, from, item.build_time);
            }
        }
        for (store, asked) in self.stores.iter_mut().zip(&asked) {
            store.rate = store
                .amount
                .iter()
                .zip(asked)
                .filter(|&(_, &ask)| ask > 0)
                .map(|(&have, &ask)| (have * 100 / ask).min(100))
                .min()
                .unwrap_or(100);
        }
        let mut finished = Vec::new();
        for sp in spenders {
            let store = self.stores.binary_search_by_key(&self.units[sp.unit].owner, |s| s.owner).ok();
            let (rate, have) = match store {
                Some(s) => (self.stores[s].rate, self.stores[s].amount.clone()),
                // Nothing to spend, so only things that cost nothing get built.
                None => (100, Vec::new()),
            };
            let item = &self.types[sp.item].production;
            let time = item.build_time;
            if let Work::Frame(t) = sp.on
                && self.units[t].build.is_none()
            {
                continue; // Another builder finished this frame earlier in the tick.
            }
            let from = self.work_of(sp.on);
            let mut to = (from + sp.power * rate / 100).min(time);
            for r in 0..item.cost.len() {
                let budget = have.get(r).copied().unwrap_or(0);
                to = to.min(affordable(item.cost_of(r), from, time, budget));
            }
            if let Some(s) = store {
                for (r, amount) in self.stores[s].amount.iter_mut().enumerate() {
                    let cost = item.cost_of(r);
                    *amount -= paid(cost, to, time) - paid(cost, from, time);
                }
            }
            match sp.on {
                Work::Queue(i) => {
                    let u = &mut self.units[i];
                    u.work = to;
                    if to == time {
                        let job = u.queue.remove(0);
                        u.work = 0;
                        if job.repeat {
                            u.queue.push(job.clone());
                        }
                        finished.push((i, job.kind));
                    }
                }
                Work::Frame(t) => {
                    // A frame's health grows with the work done on it, on top of any damage it has taken.
                    let max = i64::from(self.types[sp.item].max_health);
                    let u = &mut self.units[t];
                    u.health = (i64::from(u.health) + max * to / time - max * from / time).min(max) as i32;
                    u.build = Some(to);
                    if to == time {
                        u.build = None;
                        events.push(Event::Built { by: self.units[sp.unit].id, unit: self.units[t].id });
                    }
                }
            }
        }
        for (i, kind) in finished {
            let f = &self.units[i];
            let gap = self.half_extent(f.kind, 1) + self.types[kind].movement.radius + CONTACT;
            let (by, owner, x, y) = (f.id, f.owner, f.pos.x, f.pos.y + gap);
            let rally = f.rally;
            let unit = self.spawn_for(owner, kind, x, y);
            if let Some((rx, ry)) = rally {
                let last = self.units.len() - 1;
                self.set_goal(last, rx, ry);
            }
            events.push(Event::Built { by, unit });
        }
    }

    /// Every builder next to the wreck it is reclaiming, in id order, adds its build power to the reclaim work and
    /// gains what that work frees, as `paid` reckons it, so a wreck yields exactly its worth. A wreck is gone once
    /// all of it is reclaimed.
    fn reclaim(&mut self, events: &mut Vec<Event>) {
        for i in 0..self.units.len() {
            let u = &self.units[i];
            let power = self.types[u.kind].production.build_power;
            let Some(id) = u.reclaim else { continue };
            let Ok(w) = self.wrecks.binary_search_by_key(&id, |w| w.id) else { continue };
            if power == 0 || !self.near_wreck(i, w) {
                continue;
            }
            let wreck = &self.wrecks[w];
            let worth = &self.types[wreck.kind].production;
            let time = reclaim_time(worth.build_time);
            let (from, to) = (wreck.work, (wreck.work + power).min(time));
            if let Ok(s) = self.stores.binary_search_by_key(&u.owner, |s| s.owner) {
                for (have, &value) in self.stores[s].amount.iter_mut().zip(&worth.wreck) {
                    *have += paid(value, to, time) - paid(value, from, time);
                }
            }
            self.wrecks[w].work = to;
            if to == time {
                let gone = self.wrecks.remove(w);
                events.push(Event::Reclaimed { by: self.units[i].id, wreck: id });
                // The ground under a structure's wreck opens again.
                if self.types[gone.kind].structure.is_some() {
                    self.reblock();
                }
            }
        }
    }

    /// What unit `i` is building this tick, if anything: the first item in its queue, or the frame it is
    /// assisting if it is close enough.
    fn spender(&self, i: usize) -> Option<Spender> {
        let u = &self.units[i];
        let power = self.types[u.kind].production.build_power;
        if u.build.is_some() || power == 0 {
            return None;
        }
        if let Some(head) = u.queue.first() {
            return Some(Spender { unit: i, on: Work::Queue(i), item: head.kind, power });
        }
        let t = self.index_of(u.assist?)?;
        let frame = &self.units[t];
        (frame.build.is_some() && self.in_reach(i, t)).then_some(Spender {
            unit: i,
            on: Work::Frame(t),
            item: frame.kind,
            power,
        })
    }

    fn work_of(&self, on: Work) -> i64 {
        match on {
            Work::Queue(i) => self.units[i].work,
            Work::Frame(t) => self.units[t].build.unwrap_or(0),
        }
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Move { unit, x, y } => {
                let Some(i) = self.index_of(unit) else { return };
                let u = &mut self.units[i];
                (u.target, u.chase, u.hunt, u.plan, u.assist) = (None, false, false, None, None);
                (u.reclaim, u.patrol) = (None, None);
                self.set_goal(i, x, y);
            }
            Command::AttackMove { unit, x, y } => {
                let Some(i) = self.index_of(unit) else { return };
                let u = &mut self.units[i];
                (u.target, u.chase, u.hunt, u.plan, u.assist) = (None, false, true, None, None);
                (u.reclaim, u.patrol) = (None, None);
                self.set_goal(i, x, y);
            }
            Command::Stop { unit } => {
                if let Some(i) = self.index_of(unit) {
                    let u = &mut self.units[i];
                    (u.goal, u.rest, u.target, u.chase, u.hunt) = (None, None, None, false, false);
                    (u.plan, u.assist) = (None, None);
                    (u.reclaim, u.patrol) = (None, None);
                }
            }
            Command::Produce { unit, kind, repeat } => {
                if let Some(i) = self.index_of(unit)
                    && self.types[self.units[i].kind].production.builds.contains(&kind)
                    && self.types[kind].structure.is_none()
                {
                    self.units[i].queue.push(Job { kind, repeat });
                }
            }
            Command::Build { unit, kind, cx, cy } => self.order_build(unit, kind, cx, cy),
            Command::Assist { unit, target } => self.order_assist(unit, target),
            Command::Reclaim { unit, wreck } => self.order_reclaim(unit, wreck),
            Command::ClearQueue { unit } => {
                if let Some(i) = self.index_of(unit) {
                    (self.units[i].queue, self.units[i].work) = (Vec::new(), 0);
                }
            }
            Command::Patrol { unit, x, y } => self.order_patrol(unit, x, y),
            Command::Keep { unit, kind, count } => self.order_keep(unit, kind, count),
            Command::Rally { unit, point } => self.order_rally(unit, point),
            Command::FallBack { unit, percent, x, y } => {
                if let Some(i) = self.index_of(unit) {
                    self.units[i].fall_back = (percent > 0).then_some((percent, x, y));
                }
            }
            Command::Attack { unit, target } => {
                if let Some(i) = self.index_of(unit)
                    && unit != target
                    && self.types[self.units[i].kind].weapon.is_some()
                    && self.sees(self.units[i].owner, target)
                {
                    let u = &mut self.units[i];
                    (u.target, u.chase, u.hunt, u.plan, u.assist) = (Some(target), true, false, None, None);
                    (u.reclaim, u.patrol) = (None, None);
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
            .array("projectiles", &self.projectiles)
            .field("rng", &self.rng)
            .opt("spots", (!self.spots.is_empty()).then_some(&self.spots))
            .opt("stores", (!self.stores.is_empty()).then_some(&self.stores))
            .field("tick", &self.tick)
            .array("units", &self.units)
            .opt("vision", self.vision.as_ref())
            .opt("wrecks", (!self.wrecks.is_empty()).then_some(&self.wrecks))
            .end();
    }
}

/// A unit building something this tick: `unit` adds `power` to the work on `on`, an item of type `item`.
struct Spender {
    unit: usize,
    on: Work,
    item: usize,
    power: i64,
}

/// Where build work goes: the first item in a factory's queue, or a frame.
#[derive(Clone, Copy)]
enum Work {
    Queue(usize),
    Frame(usize),
}

/// One damage record: who is hurt, by whom, and how much.
struct Hurt {
    unit: u32,
    by: u32,
    amount: i32,
}

/// The share of splash damage a unit takes from its own side's shots, in percent.
const FRIENDLY_SPLASH_PERCENT: i32 = 50;

/// Idle armed units look for an enemy once every this many ticks, staggered by id.
const SCAN_EVERY: u32 = 8;

/// How many of the nearest enemies in range an idle unit tests for a clear shot when picking a target.
const SIGHT_TRIES: usize = 4;

/// What a projectile meets on one stretch of its flight.
enum Contact {
    Clear,
    OffMap,
    /// It bursts here, on a unit or (when `None`) on the ground.
    Struck(Vec3, Option<u32>),
}

/// Projectiles are checked against the ground and units at least this often along their path, in sub-cell
/// units.
const CHECK_EVERY: i32 = 32;

/// How close, beyond touching, a moving unit must come to a resting one to stop beside it.
const CONTACT: i32 = 8;

/// Directions to part two units standing on exactly the same point, clockwise from north.
const PART: [(i64, i64); 8] = [(0, -16), (11, -11), (16, 0), (11, 11), (0, 16), (-11, 11), (-16, 0), (-11, -11)];

/// The cell holding a point on the map.
fn cell_of(map: &Heightmap, (x, y): (i32, i32)) -> (i32, i32) {
    ((x / SUB).clamp(0, map.width() - 1), (y / SUB).clamp(0, map.height() - 1))
}

/// The cell a mover at `(x, y)` steers from. A point exactly on a cell edge touches the cells on both sides; it
/// counts as in whichever is cheaper to reach the goal from (ties to the cell `cell_of` gives), so a route and
/// its mirror image step the same way.
fn route_cell(map: &Heightmap, field: &FlowField, (x, y): (i32, i32)) -> (i32, i32) {
    let (cx, cy) = cell_of(map, (x, y));
    let xs = if x % SUB == 0 && cx > 0 { vec![cx, cx - 1] } else { vec![cx] };
    let ys = if y % SUB == 0 && cy > 0 { vec![cy, cy - 1] } else { vec![cy] };
    let mut best = (cx, cy);
    let mut best_cost = field.cost(best);
    for &ny in &ys {
        for &nx in &xs {
            let c = field.cost((nx, ny));
            if c.is_some() && best_cost.is_none_or(|b| c < Some(b)) {
                (best, best_cost) = ((nx, ny), c);
            }
        }
    }
    best
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
        let cell = route_cell(map, field, here);
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
