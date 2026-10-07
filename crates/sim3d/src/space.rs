//! Fixed-point positions in continuous space.

use rts_core::hash::{Canon, CanonHasher};
use rts_core::imath::isqrt;

/// Sub-cell units per map cell. A unit's position is exact to 1/256 of a cell.
pub const SUB: i32 = 256;

/// A point in the world: `x` and `y` across the map in sub-cell units, `z` up in height units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Vec3 {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Vec3 {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The point `num / den` of the way from `self` to `to`, rounded towards negative infinity on each axis.
    /// Intermediate products use 64 bits, so any two points on a map are safe.
    pub fn lerp(self, to: Vec3, num: i32, den: i32) -> Vec3 {
        debug_assert!(den > 0 && (0..=den).contains(&num));
        let step = |a: i32, b: i32| a + ((i64::from(b - a) * i64::from(num)).div_euclid(i64::from(den))) as i32;
        Vec3::new(step(self.x, to.x), step(self.y, to.y), step(self.z, to.z))
    }

    /// Straight-line distance across the map, ignoring height, rounded down. Units are sub-cell units.
    pub fn ground_distance(self, to: Vec3) -> u32 {
        let (dx, dy) = (i64::from(to.x - self.x), i64::from(to.y - self.y));
        isqrt((dx * dx + dy * dy) as u64) as u32
    }
}

impl Canon for Vec3 {
    fn canon(&self, w: &mut CanonHasher) {
        w.object().field("x", &self.x).field("y", &self.y).field("z", &self.z).end();
    }
}

#[cfg(test)]
mod tests {
    use super::Vec3;
    use rts_core::hash::hash_of;

    #[test]
    fn lerp_hits_both_ends_and_rounds_down() {
        let a = Vec3::new(0, 0, 0);
        let b = Vec3::new(10, -10, 3);
        assert_eq!(a.lerp(b, 0, 4), a);
        assert_eq!(a.lerp(b, 4, 4), b);
        assert_eq!(a.lerp(b, 1, 4), Vec3::new(2, -3, 0));
    }

    #[test]
    fn ground_distance_ignores_height_and_rounds_down() {
        let a = Vec3::new(0, 0, 0);
        assert_eq!(a.ground_distance(Vec3::new(3, 4, 99)), 5);
        assert_eq!(a.ground_distance(Vec3::new(1, 1, 0)), 1);
        assert_eq!(a.ground_distance(Vec3::new(-300, 400, 0)), 500);
    }

    #[test]
    fn hash_is_the_canonical_json_of_the_point() {
        let mut expected = rts_core::hash::CanonHasher::new();
        expected.raw(r#"{"x":1,"y":-2,"z":3}"#);
        assert_eq!(hash_of(&Vec3::new(1, -2, 3)).value(), expected.value());
    }
}
