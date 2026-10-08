//! What to draw, as boxes in view space. Every unit is a box the size of its body for now (a disc's square for a
//! mobile unit, the footprint for a structure, the height from its type); models from the art studio replace them
//! later. Mobile units and projectiles slide between their positions at the last two ticks, so motion is smooth at
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
}

/// An axis-aligned box from `min` to `max` in view space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub part: Part,
    pub min: V3,
    pub max: V3,
    pub colour: [u8; 4],
}

/// Size of a projectile's box, in cells.
const SHOT: f32 = 0.12;
/// Projectiles' colour.
const SHOT_COLOUR: [u8; 4] = [255, 230, 150, 255];
const WRECK_COLOUR: [u8; 4] = [70, 66, 60, 255];
const SPOT_COLOUR: [u8; 4] = [210, 170, 60, 255];

/// Turns a world into shapes, remembering where things were at the last tick so it can draw them in between.
#[derive(Clone, Debug, Default)]
pub struct Shapes {
    before: BTreeMap<u32, Vec3>,
}

impl Shapes {
    /// Call just before each `World::step`, so the next frames can slide from these positions to the new ones.
    pub fn remember(&mut self, world: &World) {
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
            out.push(Shape {
                part,
                min: [x - 0.4, y - 0.4, z],
                max: [x + 0.4, y + 0.4, z + 0.03],
                colour: SPOT_COLOUR,
            });
        }
        for w in world.wrecks() {
            let r = world.types()[w.kind].movement.radius as f32 / SUB as f32;
            let p = to_view(w.pos);
            let max = [p[0] + r, p[1] + r, p[2] + 0.1];
            out.push(Shape { part: Part::Wreck(w.id), min: [p[0] - r, p[1] - r, p[2]], max, colour: WRECK_COLOUR });
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
                let (part, colour, tall) = match u.build {
                    Some(done) => {
                        let share = (done as f32 / t.production.build_time.max(1) as f32).clamp(0.1, 1.0);
                        (Part::Frame(u.id), [pale(r), pale(g), pale(b), 255], tall * share)
                    }
                    None => (Part::Unit(u.id), [r, g, b, 255], tall),
                };
                out.push(Shape { part, min: [x0 as f32, y0 as f32, z], max: [x1, y1, z + tall], colour });
            } else {
                let now = to_view(u.pos);
                let p = self.before.get(&u.id).map_or(now, |&b| between(to_view(b), now, alpha));
                let half = t.movement.radius as f32 / SUB as f32;
                let min = [p[0] - half, p[1] - half, p[2]];
                out.push(Shape {
                    part: Part::Unit(u.id),
                    min,
                    max: [p[0] + half, p[1] + half, p[2] + tall],
                    colour: [r, g, b, 255],
                });
            }
        }
        for s in world.projectiles() {
            let p = between(to_view(s.at((s.flown - 1).max(0))), to_view(s.at(s.flown)), alpha);
            let h = SHOT / 2.0;
            let (min, max) = ([p[0] - h, p[1] - h, p[2] - h], [p[0] + h, p[1] + h, p[2] + h]);
            out.push(Shape { part: Part::Projectile(s.id), min, max, colour: SHOT_COLOUR });
        }
        out
    }
}

fn between(a: V3, b: V3, alpha: f32) -> V3 {
    [a[0] + (b[0] - a[0]) * alpha, a[1] + (b[1] - a[1]) * alpha, a[2] + (b[2] - a[2]) * alpha]
}

/// Half way to white.
fn pale(c: u8) -> u8 {
    c / 2 + 128
}
