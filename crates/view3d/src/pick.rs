//! The ground point under a screen pixel: the ray from the camera through the pixel, walked across the map until
//! it first goes under the ground, then narrowed down by halving. It tests against the same integer heights the
//! simulation uses, so an order lands where the ground really is, even where the drawn mesh cuts a corner.

use crate::ground;
use crate::maths::{V3, add, normalize, scale};
use sim3d::terrain::Heightmap;

/// How far the walk moves along the ray each step, in cells. Finer than any hill on a map.
const STEP: f32 = 0.25;

/// How many times the last step is halved: 2^-16 of a quarter cell is far below one sub-cell unit.
const HALVINGS: u32 = 16;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: V3,
    pub dir: V3,
}

impl Ray {
    pub fn at(&self, t: f32) -> V3 {
        add(self.origin, scale(self.dir, t))
    }
}

/// The first point where `ray` meets the ground on the map, or `None` if it misses the map.
pub fn ground_hit(map: &Heightmap, ray: &Ray) -> Option<V3> {
    let ray = Ray { origin: ray.origin, dir: normalize(ray.dir) };
    let (low, high) = heights(map);
    // Clip the ray to the box the ground lies in.
    let bounds = [(0.0, map.width() as f32), (0.0, map.height() as f32), (low, high)];
    let (mut enter, mut leave) = (0.0f32, f32::INFINITY);
    for (axis, &(min, max)) in bounds.iter().enumerate() {
        let (o, d) = (ray.origin[axis], ray.dir[axis]);
        if d.abs() < 1e-9 {
            if o < min || o > max {
                return None;
            }
            continue;
        }
        let (a, b) = ((min - o) / d, (max - o) / d);
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
    }
    if enter > leave {
        return None;
    }
    let above = |t: f32| {
        let p = ray.at(t);
        p[2] - ground(map, p[0], p[1])
    };
    let on_ground = |p: V3| [p[0], p[1], ground(map, p[0], p[1])];
    if above(enter) <= 0.0 {
        // It came in through the side of the map below the ground's edge.
        return Some(on_ground(ray.at(enter)));
    }
    let mut t = enter;
    while t < leave {
        let next = (t + STEP).min(leave);
        if above(next) <= 0.0 {
            let (mut a, mut b) = (t, next);
            for _ in 0..HALVINGS {
                let mid = (a + b) / 2.0;
                if above(mid) <= 0.0 { b = mid } else { a = mid }
            }
            return Some(on_ground(ray.at(b)));
        }
        t = next;
    }
    None
}

/// The lowest and highest ground on the map, in view units.
fn heights(map: &Heightmap) -> (f32, f32) {
    let (mut low, mut high) = (i32::MAX, i32::MIN);
    for cy in 0..=map.height() {
        for cx in 0..=map.width() {
            let z = map.corner_height(cx, cy);
            (low, high) = (low.min(z), high.max(z));
        }
    }
    (low as f32 / crate::HEIGHT_PER_CELL, high as f32 / crate::HEIGHT_PER_CELL)
}
