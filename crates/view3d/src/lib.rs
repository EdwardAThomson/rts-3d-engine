//! The view side of the 3D engine, kept apart from any GPU code so it can be tested on its own and drawn by the
//! wgpu renderer once the platform layer shared with the Classic engine is its own crate (docs/view.md).
//!
//! - `camera`: a camera that zooms from one unit to the whole map, tilting to look straight down as it goes.
//! - `terrain`: the triangle mesh of the heightmap, at full detail or coarser for far views.
//! - `pick`: the ground point under a screen pixel, for orders given with the mouse.
//!
//! This is not simulation code, so it uses floating point: nothing here feeds back into the state, and any order a
//! player gives from it goes in as integer sub-cell units through `World::command`.
//!
//! View space is measured in cells: `x` east and `y` south across the map, as in the simulation, and `z` up.

pub mod camera;
pub mod maths;
pub mod pick;
pub mod terrain;

use sim3d::space::{SUB, Vec3};
use sim3d::terrain::Heightmap;

/// How many height units make one cell of height when drawn. Set equal to `SUB`, so a height unit is as tall as a
/// sub-cell unit is long and slopes look as steep as the movement rules treat them.
pub const HEIGHT_PER_CELL: f32 = SUB as f32;

/// A simulation point in view space.
pub fn to_view(p: Vec3) -> [f32; 3] {
    [p.x as f32 / SUB as f32, p.y as f32 / SUB as f32, p.z as f32 / HEIGHT_PER_CELL]
}

/// A view-space point on the map in sub-cell units, rounded to the nearest and kept on the map, ready for an order.
pub fn to_sub(map: &Heightmap, p: [f32; 3]) -> (i32, i32) {
    let round = |v: f32, cells: i32| ((v * SUB as f32).round() as i32).clamp(0, cells * SUB);
    (round(p[0], map.width()), round(p[1], map.height()))
}

/// Ground height in view units at a view-space point.
pub fn ground(map: &Heightmap, x: f32, y: f32) -> f32 {
    let sub = |v: f32| (v * SUB as f32).round() as i32;
    map.sample(sub(x), sub(y)) as f32 / HEIGHT_PER_CELL
}
