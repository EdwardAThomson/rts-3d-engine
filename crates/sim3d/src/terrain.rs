//! Heightmap terrain. Heights are stored at cell corners and blended across each cell with integer bilinear
//! interpolation, so the ground is continuous and every sample is exact and repeatable.

use crate::space::{SUB, Vec3};
use rts_core::hash::{Canon, CanonHasher};

/// Terrain heights at the `(width + 1) * (height + 1)` corners of a `width` by `height` grid of cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heightmap {
    width: i32,
    height: i32,
    corners: Vec<i32>,
}

impl Heightmap {
    /// A heightmap from corner heights in row order. Panics if the count does not match the size.
    pub fn new(width: i32, height: i32, corners: Vec<i32>) -> Self {
        assert!(width > 0 && height > 0, "a map has at least one cell");
        assert_eq!(corners.len(), ((width + 1) * (height + 1)) as usize, "one height per cell corner");
        Self { width, height, corners }
    }

    /// A flat map with every corner at `z`.
    pub fn flat(width: i32, height: i32, z: i32) -> Self {
        Self::new(width, height, vec![z; ((width + 1) * (height + 1)) as usize])
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// The height at corner `(cx, cy)`, from 0 to the width and height inclusive.
    pub fn corner_height(&self, cx: i32, cy: i32) -> i32 {
        self.corners[(cy * (self.width + 1) + cx) as usize]
    }

    fn corner(&self, cx: i32, cy: i32) -> i64 {
        i64::from(self.corners[(cy * (self.width + 1) + cx) as usize])
    }

    /// Ground height under a point given in sub-cell units. Points off the map take the height at the nearest
    /// edge. The result rounds towards negative infinity.
    pub fn sample(&self, x: i32, y: i32) -> i32 {
        let x = x.clamp(0, self.width * SUB);
        let y = y.clamp(0, self.height * SUB);
        // The far edge belongs to the last cell, so its corner is still in range.
        let cx = (x / SUB).min(self.width - 1);
        let cy = (y / SUB).min(self.height - 1);
        let (fx, fy) = (i64::from(x - cx * SUB), i64::from(y - cy * SUB));
        let s = i64::from(SUB);
        let sum = self.corner(cx, cy) * (s - fx) * (s - fy)
            + self.corner(cx + 1, cy) * fx * (s - fy)
            + self.corner(cx, cy + 1) * (s - fx) * fy
            + self.corner(cx + 1, cy + 1) * fx * fy;
        sum.div_euclid(s * s) as i32
    }

    /// Whether the straight line from `from` to `to` stays at or above the ground. The line is checked every
    /// quarter of a cell and at both ends; the ends themselves may touch the ground.
    pub fn line_of_sight(&self, from: Vec3, to: Vec3) -> bool {
        let span = (to.x - from.x).abs().max((to.y - from.y).abs());
        let steps = (span / (SUB / 4)).max(1);
        (1..steps).all(|i| {
            let p = from.lerp(to, i, steps);
            self.sample(p.x, p.y) <= p.z
        })
    }
}

impl Canon for Heightmap {
    fn canon(&self, w: &mut CanonHasher) {
        w.object().array("corners", &self.corners).field("height", &self.height).field("width", &self.width).end();
    }
}
