//! The computer opponent for `sim3d` (docs/ai.md): a player without a mouse. It reads the world through `&World`
//! and acts only through `World::command`, so its orders are queued and logged exactly like a human's, and the
//! command log replays its games with the AI switched off.
//!
//! It is deterministic: integer maths, units in id order, no clock and no randomness of its own, so the same game
//! always gets the same orders and two AIs playing each other give the same hash every run.
//!
//! It knows no unit by name. What a unit type is for comes from its data: a mobile unit with build power that can
//! build structures is a builder, a structure that builds mobile units is a factory, a structure that produces a
//! resource or extracts from spots is income, and a mobile unit with a weapon is a fighter. So any setting whose
//! data has those things gets an opponent, whatever its factions, resources or units are called.
//!
//! This first version has two parts sharing a small memory (`Ai`):
//! - the base (`base.rs`): builders keep every resource coming in, build factories while there is money to
//!   spend, help finish frames and reclaim wrecks;
//! - the army (`army.rs`): factories keep building builders up to a count and then fighters, which gather at a
//!   rally point, defend the base, and set out in attack waves that grow each time.
//!
//! Under fog of war it reads only what its side knows (`known`): enemies its units see now and the enemy structures
//! it remembers. Until it has found an enemy it guesses the enemy is across the map from home (the point mirrored
//! through the middle, which suits the usual mirrored skirmish map) and sends its waves to scout the nearest
//! unexplored ground to that guess, as the Classic engine's opponent does.

#![deny(clippy::float_arithmetic, clippy::disallowed_types)]

mod army;
mod base;
pub mod skirmish;

use sim3d::space::SUB;
use sim3d::world::{Command, Unit, UnitType, World};

/// The numbers that set how the opponent plays. Our own starting values, to tune by self-play runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Ticks between thinks. Player `p` thinks on ticks where `(tick + 5 * p) % think_every == 0`, so two AIs
    /// rarely think on the same tick.
    pub think_every: u32,
    /// Builders wanted, counting the ones queued in factories.
    pub builders: usize,
    /// Most factories it builds.
    pub factories: usize,
    /// Most new structures under way at once (planned or still frames). Idle builders help finish them instead.
    pub projects: usize,
    /// A resource is short when its store is below this percent of capacity, or below half and falling.
    pub low_percent: i64,
    /// Another factory is worth building when every resource is at least this percent of capacity.
    pub rich_percent: i64,
    /// Most entries kept in one factory's queue.
    pub factory_queue: usize,
    /// Fighters in the first wave; each later wave adds `wave_growth`, up to `wave_cap`.
    pub first_wave: usize,
    pub wave_growth: usize,
    pub wave_cap: usize,
    /// A wave that falls below this percent of the fighters it set out with comes home.
    pub retreat_percent: usize,
    /// Enemy fighters this many cells from one of its structures draw out the fighters at home.
    pub defend_radius: i32,
    /// How far from home, towards the nearest enemy, new fighters gather, in cells.
    pub rally_distance: i32,
    /// How far from home it looks for building sites, in cells.
    pub search_radius: i32,
    /// How far from home builders go to reclaim wrecks, in cells.
    pub reclaim_radius: i32,
}

impl Settings {
    /// The normal opponent.
    pub fn normal() -> Settings {
        Settings {
            think_every: 15,
            builders: 3,
            factories: 3,
            projects: 2,
            low_percent: 20,
            rich_percent: 50,
            factory_queue: 2,
            first_wave: 6,
            wave_growth: 2,
            wave_cap: 16,
            retreat_percent: 30,
            defend_radius: 12,
            rally_distance: 8,
            search_radius: 20,
            reclaim_radius: 10,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::normal()
    }
}

/// One computer player and everything it remembers between thinks. None of it is in the world's state hash:
/// only the orders it gives are, through the command log.
#[derive(Clone, Debug)]
pub struct Ai {
    pub player: u8,
    pub settings: Settings,
    /// Where its base is, in sub-cell units: where its first unit stood at its first think.
    pub home: Option<(i32, i32)>,
    /// The attack wave out now, by unit id, and how many it set out with.
    pub wave: Vec<u32>,
    pub wave_start: usize,
    /// Fighters the next wave waits for.
    pub wave_size: usize,
    /// Waves sent so far.
    pub waves_sent: u32,
    /// Each resource's store at the last think, to tell whether it is falling.
    last_amount: Vec<i64>,
    /// Fighters queued so far, to take turns between the kinds a factory can build.
    fighters_queued: usize,
}

impl Ai {
    pub fn new(player: u8, settings: Settings) -> Ai {
        let wave_size = settings.first_wave;
        Ai {
            player,
            settings,
            home: None,
            wave: Vec::new(),
            wave_start: 0,
            wave_size,
            waves_sent: 0,
            last_amount: Vec::new(),
            fighters_queued: 0,
        }
    }

    /// Whether this AI thinks on the world's current tick.
    pub fn due(&self, world: &World) -> bool {
        (world.tick() + 5 * u32::from(self.player)).is_multiple_of(self.settings.think_every.max(1))
    }

    /// Think if it is due, and queue the orders for the next tick. Call once before each `World::step`.
    pub fn tick(&mut self, world: &mut World) {
        if self.due(world) {
            for c in self.think(world) {
                world.command(c);
            }
        }
    }

    /// Decide this think's orders. Reads the world and changes nothing in it.
    pub fn think(&mut self, world: &World) -> Vec<Command> {
        let mut out = Vec::new();
        if defeated(world, self.player) {
            return out;
        }
        if self.home.is_none() {
            self.home = world.units().iter().find(|u| u.owner == self.player).map(|u| (u.pos.x, u.pos.y));
        }
        base::think(self, world, &mut out);
        army::produce(self, world, &mut out);
        army::think(self, world, &mut out);
        self.last_amount = world.store(self.player).map(|s| s.amount.clone()).unwrap_or_default();
        out
    }

    fn home(&self) -> (i32, i32) {
        self.home.unwrap_or((0, 0))
    }
}

/// Whether a unit type is a structure.
pub fn is_structure(t: &UnitType) -> bool {
    t.structure.is_some()
}

/// Whether a unit type is a builder: it moves, and has build power and structures to build.
pub fn is_builder(types: &[UnitType], t: &UnitType) -> bool {
    !is_structure(t) && t.production.build_power > 0 && t.production.builds.iter().any(|&k| is_structure(&types[k]))
}

/// Whether a unit type is a factory: a structure with build power and mobile units to build.
pub fn is_factory(types: &[UnitType], t: &UnitType) -> bool {
    is_structure(t) && t.production.build_power > 0 && t.production.builds.iter().any(|&k| !is_structure(&types[k]))
}

/// Whether a unit type is a fighter: it moves and has a weapon.
pub fn is_fighter(t: &UnitType) -> bool {
    !is_structure(t) && t.weapon.is_some()
}

/// Whether a player has lost: nothing left that is a structure, a frame or a builder. Fighters alone can't
/// rebuild, so they don't keep a player in the game.
pub fn defeated(world: &World, player: u8) -> bool {
    let types = world.types();
    !world
        .units()
        .iter()
        .any(|u| u.owner == player && (is_structure(&types[u.kind]) || is_builder(types, &types[u.kind])))
}

/// The one player of `players` not yet defeated, once all the others are.
pub fn winner(world: &World, players: &[u8]) -> Option<u8> {
    let mut left = players.iter().copied().filter(|&p| !defeated(world, p));
    match (left.next(), left.next()) {
        (Some(p), None) => Some(p),
        _ => None,
    }
}

/// Whether a unit has nothing to do: no move, target, plan, frame to help or wreck to reclaim.
fn idle(u: &Unit) -> bool {
    u.goal.is_none() && u.target.is_none() && u.plan.is_none() && u.assist.is_none() && u.reclaim.is_none()
}

/// Squared ground distance between two points in sub-cell units.
fn dist2((ax, ay): (i32, i32), (bx, by): (i32, i32)) -> i64 {
    let (dx, dy) = (i64::from(ax - bx), i64::from(ay - by));
    dx * dx + dy * dy
}

/// The cell a point in sub-cell units is in.
fn cell((x, y): (i32, i32)) -> (i32, i32) {
    (x.div_euclid(SUB), y.div_euclid(SUB))
}

fn pos(u: &Unit) -> (i32, i32) {
    (u.pos.x, u.pos.y)
}

/// An enemy this player knows of: one its units see now, or a structure it remembers from earlier.
#[derive(Clone, Copy, Debug)]
struct Known {
    id: u32,
    kind: usize,
    pos: (i32, i32),
}

/// Every enemy `me` knows of, in id order. Without fog of war that is every enemy.
fn known(world: &World, me: u8) -> Vec<Known> {
    let mut out: Vec<Known> = (world.units().iter())
        .filter(|u| u.owner != me && world.sees(me, u.id))
        .map(|u| Known { id: u.id, kind: u.kind, pos: pos(u) })
        .collect();
    if let Some(v) = world.vision() {
        for g in v.ghosts(me) {
            if out.binary_search_by_key(&g.id, |k| k.id).is_err() {
                out.push(Known { id: g.id, kind: g.kind, pos: (g.pos.x, g.pos.y) });
            }
        }
        out.sort_unstable_by_key(|k| k.id);
    }
    out
}

/// Where `ai` guesses the enemy is before it has found one: home mirrored through the middle of the map.
fn guess(ai: &Ai, world: &World) -> (i32, i32) {
    let (x, y) = ai.home();
    (world.map().width() * SUB - 1 - x, world.map().height() * SUB - 1 - y)
}

/// Where to look for an enemy not yet found: the centre of the cell, on a grid every `SCOUT_GRID` cells, that
/// `ai`'s side has never seen, nearest the guess (ties to row order); failing that, one it does not see now; failing
/// that, the guess.
fn scout(ai: &Ai, world: &World) -> (i32, i32) {
    let target = guess(ai, world);
    let Some(v) = world.vision() else { return target };
    let (w, h) = (world.map().width(), world.map().height());
    let mut best: Option<((sim3d::vision::CellView, i64), (i32, i32))> = None;
    for cy in (SCOUT_GRID / 2..h).step_by(SCOUT_GRID as usize) {
        for cx in (SCOUT_GRID / 2..w).step_by(SCOUT_GRID as usize) {
            let view = v.cell(ai.player, cx, cy);
            if view == sim3d::vision::CellView::Visible {
                continue;
            }
            let p = (cx * SUB + SUB / 2, cy * SUB + SUB / 2);
            let key = (view, dist2(p, target));
            if best.is_none_or(|(b, _)| key < b) {
                best = Some((key, p));
            }
        }
    }
    best.map_or(target, |(_, p)| p)
}

/// Spacing of the points the opponent scouts, in cells.
const SCOUT_GRID: i32 = 4;
