//! Weapons and the projectiles they fire. Every shot is a real projectile flying through the world, so hills
//! and units in the way stop it, and a miss lands somewhere. That is the core of the Total Annihilation idea
//! (plans/rts-3d/inspirations.md); the code is our own.
//!
//! A projectile's flight is worked out once, when it is fired, as a closed form of the tick count: its ground
//! position moves evenly from the muzzle to the aim point and its height follows a parabola under the weapon's
//! gravity. So it passes exactly through the aim point at the end of its flight unless something is in the
//! way first, and no error builds up from tick to tick. A shot that misses flies on until it meets the ground.

use crate::space::Vec3;
use rts_core::hash::{Canon, CanonHasher};

/// A weapon, read from data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Weapon {
    /// Greatest ground distance from the firer to the target, in sub-cell units.
    pub range: i32,
    /// Ticks between shots.
    pub reload: i32,
    /// Ground distance the projectile covers each tick, in sub-cell units.
    pub speed: i32,
    /// Height units the projectile's climb rate drops by each tick. 0 is direct fire: a straight line, fired
    /// only with a clear line of sight. More than 0 lobs the shot in an arc, over hills it cannot see past.
    pub gravity: i32,
    /// Health taken from the unit struck, and from units near the impact (see `splash`).
    pub damage: i32,
    /// Units whose centre is within half this ground distance of the impact take full damage, within all of it
    /// half damage. 0 means only a unit struck directly is hurt. The firer is never hurt by its own shot.
    pub splash: i32,
    /// Random aim error: the aim point moves by up to this ground distance, drawn from the game's generator.
    pub scatter: i32,
}

/// A projectile in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Projectile {
    pub id: u32,
    /// The unit that fired it, which it never hits.
    pub firer: u32,
    /// The firer's unit type, whose weapon this is.
    pub kind: usize,
    pub from: Vec3,
    pub aim: Vec3,
    /// Ticks flown so far, and the ticks the whole flight takes.
    pub flown: i32,
    pub flight: i32,
    /// Climb rate on leaving the muzzle, in height units per tick.
    pub climb: i32,
    pub gravity: i32,
}

impl Projectile {
    /// A projectile leaving `from` towards `aim` with this weapon's speed and gravity, its climb chosen so it
    /// comes down exactly on the aim point.
    pub fn launch(id: u32, firer: u32, kind: usize, weapon: &Weapon, from: Vec3, aim: Vec3) -> Self {
        let distance = i64::from(from.ground_distance(aim));
        let speed = i64::from(weapon.speed.max(1));
        let flight = ((distance + speed - 1) / speed).max(1);
        // Height after t ticks is from.z + climb * t - gravity * t * (t + 1) / 2; solve for landing at aim.z.
        let drop = i64::from(weapon.gravity) * flight * (flight + 1) / 2;
        let climb = (i64::from(aim.z - from.z) + drop).div_euclid(flight);
        Self {
            id,
            firer,
            kind,
            from,
            aim,
            flown: 0,
            flight: flight as i32,
            climb: climb as i32,
            gravity: weapon.gravity,
        }
    }

    /// Where the projectile is after `t` ticks of flight. It is exactly on the aim point after `flight` ticks,
    /// and keeps going the same way after that if nothing has stopped it.
    pub fn at(&self, t: i32) -> Vec3 {
        if t == self.flight {
            return self.aim;
        }
        let (t, n) = (i64::from(t), i64::from(self.flight));
        let along = |a: i32, b: i32| a + (i64::from(b - a) * t).div_euclid(n) as i32;
        let (climb, g) = (i64::from(self.climb), i64::from(self.gravity));
        let z = i64::from(self.from.z) + climb * t - g * t * (t + 1) / 2;
        Vec3::new(along(self.from.x, self.aim.x), along(self.from.y, self.aim.y), z as i32)
    }
}

impl Canon for Projectile {
    fn canon(&self, w: &mut CanonHasher) {
        w.object()
            .field("aim", &self.aim)
            .field("climb", &self.climb)
            .field("firer", &self.firer)
            .field("flight", &self.flight)
            .field("flown", &self.flown)
            .field("from", &self.from)
            .field("gravity", &self.gravity)
            .field("id", &self.id)
            .field("kind", &(self.kind as u32))
            .end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(gravity: i32) -> Weapon {
        Weapon { range: 2048, reload: 30, speed: 64, gravity, damage: 40, splash: 0, scatter: 0 }
    }

    #[test]
    fn a_lobbed_shot_rises_and_lands_exactly_on_the_aim_point() {
        let from = Vec3::new(0, 0, 10);
        let aim = Vec3::new(1000, 0, 50);
        let p = Projectile::launch(1, 1, 0, &shell(2), from, aim);
        assert_eq!(p.flight, 16, "1000 at 64 a tick, rounded up");
        assert_eq!(p.at(0), from);
        assert_eq!(p.at(16), aim);
        let top = (0..=16).map(|t| p.at(t).z).max().unwrap();
        assert_eq!(top, 91, "the arc climbs well above both ends");
        // Ground position moves evenly, and carries on past the aim point.
        assert_eq!(p.at(8).x, 500);
        assert_eq!(p.at(32).x, 2000);
        assert!(p.at(17).z < 50, "past the aim point it keeps falling");
    }

    #[test]
    fn direct_fire_flies_in_a_straight_line() {
        let from = Vec3::new(0, 0, 0);
        let aim = Vec3::new(640, 0, 100);
        let p = Projectile::launch(1, 1, 0, &shell(0), from, aim);
        assert_eq!(p.flight, 10);
        for t in 0..=10 {
            assert_eq!(p.at(t), Vec3::new(64 * t, 0, 10 * t));
        }
    }

    #[test]
    fn a_point_blank_shot_still_takes_one_tick() {
        let p = Projectile::launch(1, 1, 0, &shell(3), Vec3::new(5, 5, 5), Vec3::new(5, 5, 0));
        assert_eq!((p.flight, p.at(1)), (1, Vec3::new(5, 5, 0)));
    }
}
