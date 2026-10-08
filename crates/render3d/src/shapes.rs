//! What to draw, as boxes in view space. Every unit is a box the size of its body (a disc's square for a mobile
//! unit, the footprint for a structure, the height from its type), carrying its kind and which way it faces, so the
//! renderer can draw the kind's model there instead when it has one. Mobile units and projectiles slide between their positions at the last two ticks, so motion is smooth at
//! any frame rate while the simulation keeps its fixed tick.

use std::collections::BTreeMap;

use sim3d::space::{SUB, Vec3};
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
    Projectile(u32),
    Wreck(u32),
    /// A resource spot, by cell.
    Spot(i32, i32),
    /// Where a structure is about to be placed, by its north-west cell: drawn while placing it.
    Site(i32, i32),
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

/// Size of a projectile's box, in cells.
const SHOT: f32 = 0.12;
/// Projectiles' colour.
const SHOT_COLOUR: [u8; 4] = [255, 230, 150, 255];
const WRECK_COLOUR: [u8; 4] = [70, 66, 60, 255];
/// How tall a structure's wreck is, as a share of the structure.
pub const WRECK_SHARE: f32 = 0.35;
/// How tall a mobile unit's wreck is, in cells: low enough to drive over.
pub const HEAP: f32 = 0.08;
const SPOT_COLOUR: [u8; 4] = [210, 170, 60, 255];

/// Turns a world into shapes, remembering where things were at the last tick so it can draw them in between.
#[derive(Clone, Debug, Default)]
pub struct Shapes {
    before: BTreeMap<u32, Vec3>,
    /// Which way each mobile unit last moved, kept while it stands still.
    headings: BTreeMap<u32, f32>,
}

/// The heading of a move from `a` to `b` (in the terms of `Shape::yaw`), if it moved at all.
fn heading(a: Vec3, b: Vec3) -> Option<f32> {
    let (dx, dy) = ((b.x - a.x) as f32, (b.y - a.y) as f32);
    (dx != 0.0 || dy != 0.0).then(|| dx.atan2(-dy))
}

impl Shapes {
    /// Call just before each `World::step`, so the next frames can slide from these positions to the new ones.
    pub fn remember(&mut self, world: &World) {
        for u in world.units() {
            if let Some(h) = self.before.get(&u.id).and_then(|&b| heading(b, u.pos)) {
                self.headings.insert(u.id, h);
            }
        }
        self.headings.retain(|id, _| world.unit(*id).is_some());
        self.before = world.units().iter().map(|u| (u.id, u.pos)).collect();
    }

    /// The shapes for `world`, `alpha` (0 to 1) of the way from the tick before to this one.
    pub fn shapes(&self, world: &World, alpha: f32) -> Vec<Shape> {
        let map = world.map();
        let mut out = Vec::new();
        for spot in world.spots() {
            let (x, y) = (spot.cx as f32 + 0.5, spot.cy as f32 + 0.5);
            let z = ground(map, x, y);
            let part = Part::Spot(spot.cx, spot.cy);
            out.push(Shape::plain(part, [x - 0.4, y - 0.4, z], [x + 0.4, y + 0.4, z + 0.03], SPOT_COLOUR));
        }
        // Wrecks: a structure's covers the footprint it blocks, a mobile unit's is a low heap that others drive over.
        // Each lies at its own angle, which only looks matter, so it comes from the wreck's id.
        let mut heaps = Vec::new();
        for w in world.wrecks() {
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
        for u in world.units() {
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
                let aim = self.aim(world, u, 0.0);
                let unit = Pose { kind: u.kind, structure: true, yaw: 0.0, aim, grown };
                out.push(Shape { part, min, max, colour, unit: Some(unit) });
            } else {
                let now = to_view(u.pos);
                let mut p = self.before.get(&u.id).map_or(now, |&b| between(to_view(b), now, alpha));
                if t.movement.altitude == 0 {
                    p[2] += over_heaps(&heaps, p, t.movement.radius as f32 / SUB as f32);
                }
                let half = t.movement.radius as f32 / SUB as f32;
                let min = [p[0] - half, p[1] - half, p[2]];
                // A unit that has never moved faces the middle of the map, as it would set out.
                let yaw = self.headings.get(&u.id).copied().unwrap_or_else(|| {
                    let (w, h) = (map.width() * SUB / 2, map.height() * SUB / 2);
                    heading(u.pos, Vec3 { x: w, y: h, z: 0 }).unwrap_or(0.0)
                });
                out.push(Shape {
                    part: Part::Unit(u.id),
                    min,
                    max: [p[0] + half, p[1] + half, p[2] + tall],
                    colour: [r, g, b, 255],
                    unit: Some(Pose { kind: u.kind, structure: false, yaw, aim: self.aim(world, u, yaw), grown: 1.0 }),
                });
            }
        }
        for s in world.projectiles() {
            let p = between(to_view(s.at((s.flown - 1).max(0))), to_view(s.at(s.flown)), alpha);
            let h = SHOT / 2.0;
            let (min, max) = ([p[0] - h, p[1] - h, p[2] - h], [p[0] + h, p[1] + h, p[2] + h]);
            out.push(Shape::plain(Part::Projectile(s.id), min, max, SHOT_COLOUR));
        }
        out
    }

    /// Where a unit's turret points: at its target if it has one, otherwise the way it faces.
    fn aim(&self, world: &World, u: &sim3d::world::Unit, yaw: f32) -> f32 {
        u.target.and_then(|t| world.unit(t)).and_then(|t| heading(u.pos, t.pos)).unwrap_or(yaw)
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
