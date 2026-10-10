//! What to draw, as boxes in view space. Every unit is a box the size of its body (a disc's square for a mobile
//! unit, the footprint for a structure, the height from its type), carrying its kind and which way it faces, so the
//! renderer can draw the kind's model there instead when it has one. Mobile units slide between their positions at
//! the last two ticks, so motion is smooth at any frame rate while the simulation keeps its fixed tick. Shots are
//! drawn by `shots`, as effects.

use std::collections::BTreeMap;

use sim3d::space::{SUB, TURN, Vec3, angle_diff};
use sim3d::vision::CellView;
use sim3d::world::World;
use view3d::maths::V3;
use view3d::{HEIGHT_PER_CELL, ground, to_view};

/// Colours for players by owner number, wrapping round after eight. Generic, so any setting can repaint them later.
pub const PLAYER_COLOURS: [[u8; 3]; 8] = [
    [60, 110, 230],
    [220, 60, 50],
    [60, 180, 80],
    [230, 200, 50],
    [160, 80, 200],
    [60, 200, 210],
    [240, 140, 40],
    [235, 235, 235],
];

/// What a shape stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Unit(u32),
    /// A structure still being built: drawn pale, as tall as it is finished.
    Frame(u32),
    Wreck(u32),
    /// A resource spot, by cell.
    Spot(i32, i32),
    /// Where a structure is about to be placed, by its north-west cell: drawn while placing it.
    Site(i32, i32),
    /// A selected factory's rally point, by the factory's id.
    Rally(u32),
    /// An enemy structure as the viewer last saw it under fog of war, by its id; drawn, never picked.
    Ghost(u32),
}

/// An axis-aligned box from `min` to `max` in view space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub part: Part,
    pub min: V3,
    pub max: V3,
    pub colour: [u8; 4],
    /// For a unit or frame, what a model standing in for the box needs.
    pub unit: Option<Pose>,
}

/// A unit's kind and bearing, for drawing its model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub kind: usize,
    pub structure: bool,
    /// Which way it faces, in radians clockwise from north seen from above: 0 faces north (towards smaller `y`),
    /// a quarter turn faces east. Structures face north.
    pub yaw: f32,
    /// Which way its turret faces, in the same terms.
    pub aim: f32,
    /// How far a frame is built, from 0 to 1; 1 for a finished unit. A wreck is drawn squashed to `WRECK_SHARE`.
    pub grown: f32,
}

impl Shape {
    /// A plain box, with no model.
    pub fn plain(part: Part, min: V3, max: V3, colour: [u8; 4]) -> Shape {
        Shape { part, min, max, colour, unit: None }
    }
}

const WRECK_COLOUR: [u8; 4] = [70, 66, 60, 255];
/// How tall a structure's wreck is, as a share of the structure.
pub const WRECK_SHARE: f32 = 0.35;
/// How tall a mobile unit's wreck is, in cells: low enough to drive over.
pub const HEAP: f32 = 0.08;
const SPOT_COLOUR: [u8; 4] = [210, 170, 60, 255];

/// Turns a world into shapes, remembering where things were at the last tick so it can draw them in between.
#[derive(Clone, Debug, Default)]
pub struct Shapes {
    /// The player whose view of the world is drawn under fog of war: enemies it doesn't see are left out, and the
    /// enemy structures it remembers are drawn where it last saw them. `None` draws everything.
    pub viewer: Option<u8>,
    /// Each unit's position, facing and turret angle at the last tick.
    before: BTreeMap<u32, (Vec3, i32, i32)>,
}

/// A simulation angle (`sim3d::space::TURN` to a turn) in the terms of `Pose::yaw`.
pub fn radians(angle: i32) -> f32 {
    angle as f32 * std::f32::consts::TAU / TURN as f32
}

/// The angle `alpha` of the way from `a` to `b` the short way round, in radians.
fn swing(a: i32, b: i32, alpha: f32) -> f32 {
    radians(a) + radians(angle_diff(a, b)) * alpha
}

impl Shapes {
    /// Call just before each `World::step`, so the next frames can slide from these positions to the new ones.
    pub fn remember(&mut self, world: &World) {
        self.before = world.units().iter().map(|u| (u.id, (u.pos, u.facing, u.aim))).collect();
    }

    /// The shapes for `world`, `alpha` (0 to 1) of the way from the tick before to this one.
    pub fn shapes(&self, world: &World, alpha: f32) -> Vec<Shape> {
        let map = world.map();
        let mut out = Vec::new();
        let vision = self.viewer.zip(world.vision());
        // What lies on ground the viewer has never seen is unknown.
        let explored = |x: f32, y: f32| {
            vision.is_none_or(|(p, v)| v.cell(p, x.floor() as i32, y.floor() as i32) != CellView::Shroud)
        };
        for spot in world.spots().iter().filter(|s| explored(s.cx as f32, s.cy as f32)) {
            let (x, y) = (spot.cx as f32 + 0.5, spot.cy as f32 + 0.5);
            let z = ground(map, x, y);
            let part = Part::Spot(spot.cx, spot.cy);
            out.push(Shape::plain(part, [x - 0.4, y - 0.4, z], [x + 0.4, y + 0.4, z + 0.03], SPOT_COLOUR));
        }
        // Wrecks: a structure's covers the footprint it blocks, a mobile unit's is a low heap that others drive over.
        // Each lies at its own angle, which only looks matter, so it comes from the wreck's id.
        let mut heaps = Vec::new();
        for w in world.wrecks().iter().filter(|w| explored(w.pos.x as f32 / SUB as f32, w.pos.y as f32 / SUB as f32)) {
            let t = &world.types()[w.kind];
            let p = to_view(w.pos);
            let (min, max, structure) = match t.structure {
                Some(s) => {
                    let (x0, y0) = (
                        (w.pos.x - s.width * SUB / 2).div_euclid(SUB) as f32,
                        (w.pos.y - s.depth * SUB / 2).div_euclid(SUB) as f32,
                    );
                    let tall = t.height as f32 / HEIGHT_PER_CELL * WRECK_SHARE;
                    ([x0, y0, p[2]], [x0 + s.width as f32, y0 + s.depth as f32, p[2] + tall], true)
                }
                None => {
                    let r = t.movement.radius as f32 / SUB as f32;
                    heaps.push((p, r));
                    ([p[0] - r, p[1] - r, p[2]], [p[0] + r, p[1] + r, p[2] + HEAP], false)
                }
            };
            let yaw = if structure { 0.0 } else { (w.id as f32 * 2.399_963).rem_euclid(std::f32::consts::TAU) };
            let pose = Pose { kind: w.kind, structure, yaw, aim: yaw + 0.6, grown: WRECK_SHARE };
            out.push(Shape { part: Part::Wreck(w.id), min, max, colour: WRECK_COLOUR, unit: Some(pose) });
        }
        let seen = |u: &sim3d::world::Unit| self.viewer.is_none_or(|p| world.sees(p, u.id));
        for u in world.units().iter().filter(|u| seen(u)) {
            let t = &world.types()[u.kind];
            let [r, g, b] = PLAYER_COLOURS[usize::from(u.owner) % PLAYER_COLOURS.len()];
            let tall = t.height as f32 / HEIGHT_PER_CELL;
            if let Some(s) = t.structure {
                // The same cells the simulation blocks, on the ground at the footprint's centre.
                let (x0, y0) =
                    ((u.pos.x - s.width * SUB / 2).div_euclid(SUB), (u.pos.y - s.depth * SUB / 2).div_euclid(SUB));
                let (x1, y1) = ((x0 + s.width) as f32, (y0 + s.depth) as f32);
                let z = u.pos.z as f32 / HEIGHT_PER_CELL;
                let (part, colour, grown) = match u.build {
                    Some(done) => {
                        let share = (done as f32 / t.production.build_time.max(1) as f32).clamp(0.1, 1.0);
                        (Part::Frame(u.id), [pale(r), pale(g), pale(b), 255], share)
                    }
                    None => (Part::Unit(u.id), [r, g, b, 255], 1.0),
                };
                let tall = tall * grown;
                let (min, max) = ([x0 as f32, y0 as f32, z], [x1, y1, z + tall]);
                let (_, aim) = self.turned(u, alpha);
                let unit = Pose { kind: u.kind, structure: true, yaw: 0.0, aim, grown };
                out.push(Shape { part, min, max, colour, unit: Some(unit) });
            } else {
                let now = to_view(u.pos);
                let mut p = self.before.get(&u.id).map_or(now, |&(b, _, _)| between(to_view(b), now, alpha));
                if t.movement.altitude == 0 {
                    p[2] += over_heaps(&heaps, p, t.movement.radius as f32 / SUB as f32);
                }
                let half = t.movement.radius as f32 / SUB as f32;
                let min = [p[0] - half, p[1] - half, p[2]];
                let (yaw, aim) = self.turned(u, alpha);
                out.push(Shape {
                    part: Part::Unit(u.id),
                    min,
                    max: [p[0] + half, p[1] + half, p[2] + tall],
                    colour: [r, g, b, 255],
                    unit: Some(Pose { kind: u.kind, structure: false, yaw, aim, grown: 1.0 }),
                });
            }
        }
        // Enemy structures the viewer remembers but can't see now, where it last saw them.
        if let Some((p, v)) = vision {
            for g in v.ghosts(p).iter().filter(|g| world.unit(g.id).is_none_or(|u| !seen(u))) {
                let t = &world.types()[g.kind];
                let Some(st) = t.structure else { continue };
                let (x0, y0) =
                    ((g.pos.x - st.width * SUB / 2).div_euclid(SUB), (g.pos.y - st.depth * SUB / 2).div_euclid(SUB));
                let z = g.pos.z as f32 / HEIGHT_PER_CELL;
                let min = [x0 as f32, y0 as f32, z];
                let max = [(x0 + st.width) as f32, (y0 + st.depth) as f32, z + t.height as f32 / HEIGHT_PER_CELL];
                let [r, gr, b] = PLAYER_COLOURS[usize::from(g.owner) % PLAYER_COLOURS.len()];
                let pose = Pose { kind: g.kind, structure: true, yaw: 0.0, aim: 0.0, grown: 1.0 };
                out.push(Shape { part: Part::Ghost(g.id), min, max, colour: [r, gr, b, 255], unit: Some(pose) });
            }
        }
        out
    }

    /// Which ways a unit's body and turret face, `alpha` of the way from the tick before to this one, as the
    /// simulation turned them.
    fn turned(&self, u: &sim3d::world::Unit, alpha: f32) -> (f32, f32) {
        let (facing, aim) = self.before.get(&u.id).map_or((u.facing, u.aim), |&(_, f, a)| (f, a));
        let yaw = swing(facing, u.facing, alpha);
        (yaw, swing(facing + aim, u.facing + u.aim, alpha))
    }
}

/// How far a ground unit of radius `r` at `p` rides up over the wreck heaps under it: the full height of a heap
/// once its middle is over the heap's middle, rising from nothing as their edges first touch.
fn over_heaps(heaps: &[(V3, f32)], p: V3, r: f32) -> f32 {
    heaps
        .iter()
        .map(|&(h, hr)| {
            let d = ((p[0] - h[0]).powi(2) + (p[1] - h[1]).powi(2)).sqrt();
            HEAP * (1.0 - d / (r + hr)).clamp(0.0, 0.5) * 2.0
        })
        .fold(0.0, f32::max)
}

fn between(a: V3, b: V3, alpha: f32) -> V3 {
    [a[0] + (b[0] - a[0]) * alpha, a[1] + (b[1] - a[1]) * alpha, a[2] + (b[2] - a[2]) * alpha]
}

/// Half way to white.
fn pale(c: u8) -> u8 {
    c / 2 + 128
}
