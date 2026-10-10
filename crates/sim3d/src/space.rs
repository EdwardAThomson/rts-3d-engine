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

    /// The point `num / den` of the way from `self` to `to`, rounded towards `self` on each axis, so a step one
    /// way and the mirror-image step the other way cover the same ground. Intermediate products use 64 bits, so
    /// any two points on a map are safe.
    pub fn lerp(self, to: Vec3, num: i32, den: i32) -> Vec3 {
        debug_assert!(den > 0 && (0..=den).contains(&num));
        let step = |a: i32, b: i32| a + (i64::from(b - a) * i64::from(num) / i64::from(den)) as i32;
        Vec3::new(step(self.x, to.x), step(self.y, to.y), step(self.z, to.z))
    }

    /// Straight-line distance across the map, ignoring height, rounded down. Units are sub-cell units.
    pub fn ground_distance(self, to: Vec3) -> u32 {
        let (dx, dy) = (i64::from(to.x - self.x), i64::from(to.y - self.y));
        isqrt((dx * dx + dy * dy) as u64) as u32
    }
}

/// Angle units in a whole turn. Angles are bearings measured clockwise from north (towards smaller `y`) seen from
/// above, so a quarter turn (`TURN / 4`) faces east (larger `x`).
pub const TURN: i32 = 4096;
pub const HALF: i32 = TURN / 2;

/// `atan(2^-i)` in 2^-20 of a turn, for the vectoring steps of `octant`.
const ATAN: [i64; 16] = [131072, 77376, 40884, 20753, 10417, 5213, 2607, 1304, 652, 326, 163, 81, 41, 20, 10, 5];

/// `atan(b / a)` for `0 <= b <= a`, `a > 0`, in angle units (0 to `TURN / 8`), by turning the vector onto the `a`
/// axis a halving step at a time (CORDIC). Integer only.
fn octant(a: i64, b: i64) -> i32 {
    // Scale up so the shifts keep their precision; both fit in 32 bits after scaling.
    let shift = 30 - (64 - a.leading_zeros() as i32).min(30);
    let (mut x, mut y, mut z) = (a << shift, b << shift, 0i64);
    for (i, step) in ATAN.iter().enumerate() {
        let (dx, dy) = (y >> i, x >> i);
        if y > 0 {
            (x, y, z) = (x + dx, y - dy, z + step);
        } else {
            (x, y, z) = (x - dx, y + dy, z - step);
        }
    }
    ((z + 128) >> 8).clamp(0, i64::from(TURN / 8)) as i32
}

/// The bearing of a step of `dx` east and `dy` south (sub-cell units), clockwise from north in angle units, or
/// `None` for no step. Worked out in the first octant and reflected into place, so a step and its mirror image (the
/// opposite step) differ by exactly half a turn.
pub fn bearing(dx: i32, dy: i32) -> Option<i32> {
    let (east, north) = (i64::from(dx), -i64::from(dy));
    if east == 0 && north == 0 {
        return None;
    }
    let (ax, ay) = (east.abs(), north.abs());
    // From the north-south line, up to a quarter turn.
    let base = if ay >= ax { octant(ay, ax) } else { TURN / 4 - octant(ax, ay) };
    Some(match (east >= 0, north >= 0) {
        (true, true) => base,
        (true, false) => HALF - base,
        (false, false) => HALF + base,
        (false, true) => (TURN - base) % TURN,
    })
}

/// A step of `x` east and `y` south turned clockwise by `angle` angle units, as seen from above: a step east turned
/// a quarter turn points south. Whole quarter turns are exact swaps and sign changes, and the rest of the turn is
/// done by rotating a halving step at a time (CORDIC), so a step turned by an angle and by that angle plus half a
/// turn come out exactly opposite.
pub fn rotate(x: i32, y: i32, angle: i32) -> (i32, i32) {
    let angle = angle.rem_euclid(TURN);
    let (quarter, rest) = (angle / (TURN / 4), angle % (TURN / 4));
    // Rotate by the rest, at most a quarter turn, in 2^-20 of a turn, with 12 extra bits of precision.
    let (mut px, mut py, mut z) = (i64::from(x) << 12, i64::from(y) << 12, i64::from(rest) << 8);
    for (i, step) in ATAN.iter().enumerate() {
        let (dx, dy) = (py >> i, px >> i);
        if z >= 0 {
            (px, py, z) = (px - dx, py + dy, z - step);
        } else {
            (px, py, z) = (px + dx, py - dy, z + step);
        }
    }
    // The steps stretch the vector by about 1.6468; scale back by its inverse, 39797 / 2^16, rounding to nearest.
    let unstretch = |v: i64| ((v * 39797 + (1 << 27)) >> 28) as i32;
    let (rx, ry) = (unstretch(px), unstretch(py));
    match quarter {
        0 => (rx, ry),
        1 => (-ry, rx),
        2 => (-rx, -ry),
        _ => (ry, -rx),
    }
}

/// How far `to` is from `from`, the short way round: from just over minus half a turn to half a turn. Exactly
/// opposite counts as half a turn clockwise, so a unit and its mirror image turn the same way.
pub fn angle_diff(from: i32, to: i32) -> i32 {
    let d = (to - from).rem_euclid(TURN);
    if d > HALF { d - TURN } else { d }
}

/// `from` turned towards `to` by at most `rate` angle units, the short way round; 0 turns all the way at once.
pub fn turn_towards(from: i32, to: i32, rate: i32) -> i32 {
    let d = angle_diff(from, to);
    let step = if rate <= 0 { d } else { d.clamp(-rate, rate) };
    (from + step).rem_euclid(TURN)
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
    fn lerp_hits_both_ends_and_rounds_towards_the_start() {
        let a = Vec3::new(0, 0, 0);
        let b = Vec3::new(10, -10, 3);
        assert_eq!(a.lerp(b, 0, 4), a);
        assert_eq!(a.lerp(b, 4, 4), b);
        assert_eq!(a.lerp(b, 1, 4), Vec3::new(2, -2, 0));
        // A step and its mirror image cover the same ground.
        let (c, d) = (Vec3::new(100, 100, 0), Vec3::new(-100, -100, 0));
        let (e, f) = (Vec3::new(-100, -100, 0), Vec3::new(100, 100, 0));
        assert_eq!(c.lerp(d, 3, 7).x - c.x, -(e.lerp(f, 3, 7).x - e.x));
    }

    #[test]
    fn ground_distance_ignores_height_and_rounds_down() {
        let a = Vec3::new(0, 0, 0);
        assert_eq!(a.ground_distance(Vec3::new(3, 4, 99)), 5);
        assert_eq!(a.ground_distance(Vec3::new(1, 1, 0)), 1);
        assert_eq!(a.ground_distance(Vec3::new(-300, 400, 0)), 500);
    }

    #[test]
    fn bearings_point_the_right_way_and_mirror_exactly() {
        use super::{TURN, angle_diff, bearing, turn_towards};
        assert_eq!(bearing(0, 0), None);
        assert_eq!(bearing(0, -5), Some(0), "north");
        assert_eq!(bearing(5, 0), Some(TURN / 4), "east");
        assert_eq!(bearing(0, 5), Some(TURN / 2), "south");
        assert_eq!(bearing(-5, 0), Some(3 * TURN / 4), "west");
        assert_eq!(bearing(7, -7), Some(TURN / 8), "north-east");
        // A step and the opposite step are exactly half a turn apart, and bearings rise steadily round the
        // compass. (render3d's tests check them against floating point, which this crate never uses.)
        let mut last = -1;
        for i in 0..=64 {
            let b = bearing(i * 64, -4096).unwrap();
            assert!(b >= last, "{i}");
            last = b;
        }
        for dx in -40..=40 {
            for dy in -40..=40 {
                let Some(b) = bearing(dx * 37, dy * 37) else { continue };
                assert_eq!(bearing(-dx * 37, -dy * 37), Some((b + TURN / 2) % TURN));
            }
        }
        assert_eq!(angle_diff(100, 4000), -196);
        assert_eq!(angle_diff(0, TURN / 2), TURN / 2, "opposite turns clockwise");
        assert_eq!(turn_towards(100, 4000, 50), 50);
        assert_eq!(turn_towards(4090, 20, 50), 20, "the short way, across north");
        assert_eq!(turn_towards(0, 1000, 0), 1000, "0 turns at once");
        // Turning a step: quarter turns are exact, half a turn more is exactly opposite, and length is kept.
        use super::rotate;
        assert_eq!(rotate(100, 0, TURN / 4), (0, 100), "east turned clockwise points south");
        assert_eq!(rotate(0, 100, TURN / 4), (-100, 0), "south turned clockwise points west");
        assert_eq!(rotate(37, -12, 0), (37, -12));
        for angle in (0..TURN).step_by(37) {
            for (x, y) in [(500, 0), (-300, 700), (1, 1), (0, -2000)] {
                let (a, b) = rotate(x, y, angle);
                assert_eq!(rotate(x, y, angle + TURN / 2), (-a, -b), "{angle}");
                let (before, after) = (i64::from(x * x + y * y), i64::from(a * a + b * b));
                let tolerance = 4 * rts_core::imath::isqrt(before as u64) as i64 + 4;
                assert!((after - before).abs() <= tolerance, "{x} {y} {angle}: {a} {b}");
                if (angle % (TURN / 4) == 0 || (x, y) == (500, 0))
                    && let Some(want) = bearing(x, y)
                {
                    let got = bearing(a, b).unwrap();
                    assert!(angle_diff((want + angle) % TURN, got).abs() <= 3, "{x} {y} {angle}");
                }
            }
        }
    }

    #[test]
    fn hash_is_the_canonical_json_of_the_point() {
        let mut expected = rts_core::hash::CanonHasher::new();
        expected.raw(r#"{"x":1,"y":-2,"z":3}"#);
        assert_eq!(hash_of(&Vec3::new(1, -2, 3)).value(), expected.value());
    }
}
