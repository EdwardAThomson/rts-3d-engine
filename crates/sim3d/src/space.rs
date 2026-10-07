//! Fixed-point positions in continuous space.

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
}

#[cfg(test)]
mod tests {
    use super::Vec3;

    #[test]
    fn lerp_hits_both_ends_and_rounds_down() {
        let a = Vec3::new(0, 0, 0);
        let b = Vec3::new(10, -10, 3);
        assert_eq!(a.lerp(b, 0, 4), a);
        assert_eq!(a.lerp(b, 4, 4), b);
        assert_eq!(a.lerp(b, 1, 4), Vec3::new(2, -3, 0));
    }
}
