//! Effects drawn from the world's `events`: a flash at the muzzle when a unit fires, the shot itself in flight with
//! its trail (`shots`), a burst with sparks where it lands (bigger for a shell with splash), a blast where a unit is
//! destroyed, and smoke rising from wrecks, with fire at the foot of a fallen building's for a while. A building
//! going up also shakes the view for a moment (`shake`). Each is a handful of soft blobs and streaks that face the
//! camera (`Puff`), worked out with no GPU from the time since the event, so they are tested on their own. Like everything
//! here they only read: nothing in `events` feeds back into the state. Sprite-particle effects of this kind are the
//! genre's usual way; the code and numbers are ours.

use std::collections::BTreeMap;

use sim3d::world::{Event, World};
use view3d::maths::V3;
use view3d::to_view;

use crate::shots::{Flights, Looks};

/// One soft round blob, facing the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puff {
    pub at: V3,
    /// In cells.
    pub radius: f32,
    /// Its colour, and in alpha how strong it is.
    pub colour: [u8; 4],
    /// 1 for light that brightens what is behind it (fire, flashes), 0 for smoke that covers it.
    pub glow: f32,
    /// For a streak, from `at` to its other end; zero for a round blob. A streak is drawn as a capsule `radius` wide,
    /// brightest at `at` and fading towards the other end.
    pub stretch: V3,
}

impl Puff {
    /// A round blob.
    pub fn round(at: V3, radius: f32, colour: [u8; 4], glow: f32) -> Puff {
        Puff { at, radius, colour, glow, stretch: [0.0; 3] }
    }

    /// A streak from `at` along `stretch`.
    pub fn streak(at: V3, radius: f32, colour: [u8; 4], glow: f32, stretch: V3) -> Puff {
        Puff { at, radius, colour, glow, stretch }
    }
}

/// How long each effect lasts, in ticks.
pub const FLASH_TICKS: f32 = 4.0;
pub const BURST_TICKS: f32 = 16.0;
pub const BLAST_TICKS: f32 = 32.0;
/// How long a wreck smokes, in ticks: a building's longer than a vehicle's.
pub const SMOKE_TICKS: [f32; 2] = [240.0, 900.0];
/// A new puff of smoke every so many ticks, each rising for so long.
const PUFF_EVERY: f32 = 5.0;
const PUFF_LIFE: f32 = 75.0;
/// How long the ground shakes after a building goes up, in ticks, and how far off it is felt, in cells.
pub const SHAKE_TICKS: f32 = 24.0;
const SHAKE_REACH: f32 = 24.0;
/// Sparks thrown out where a shot lands.
const SPARKS: u32 = 5;
/// The most effects kept at once; the oldest go first.
const MAX_LIVE: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Flash,
    /// A shot landing; `size` is its radius in cells.
    Burst {
        size: f32,
    },
    /// A unit destroyed.
    Blast {
        size: f32,
    },
}

#[derive(Clone, Copy, Debug)]
struct Effect {
    kind: Kind,
    at: V3,
    born: f32,
    seed: u32,
}

#[derive(Clone, Copy, Debug)]
struct Smoke {
    wreck: u32,
    at: V3,
    born: f32,
    until: f32,
    size: f32,
    building: bool,
}

/// The effects playing now.
#[derive(Debug, Default)]
pub struct Effects {
    live: Vec<Effect>,
    smoke: Vec<Smoke>,
    /// Shots in flight and their splash, in sub-cell units, from when they were fired.
    splash: BTreeMap<u32, i32>,
    /// Buildings that went up lately, which shake the view: where, when and how big.
    quakes: Vec<(V3, f32, f32)>,
    /// Shots in flight and the trails they leave.
    flights: Flights,
}

impl Effects {
    /// Effects with shots drawn in the setting's looks; `Effects::default()` gives every shot the look its weapon
    /// suggests.
    pub fn with_looks(looks: Looks) -> Effects {
        Effects { flights: Flights::new(looks), ..Effects::default() }
    }

    /// Start the effects for the events of the tick just stepped; `world` is the state after it.
    pub fn observe(&mut self, world: &World, events: &[Event]) {
        // The picture runs a tick behind the state, so what happened in this step shows from the last tick on.
        let born = world.tick() as f32 - 1.0;
        self.flights.observe(world, events);
        for e in events {
            match *e {
                Event::Fired { unit, projectile, from, .. } => {
                    let splash =
                        world.unit(unit).and_then(|u| world.types()[u.kind].weapon.as_ref()).map_or(0, |w| w.splash);
                    self.splash.insert(projectile, splash);
                    self.start(Kind::Flash, to_view(from), born, projectile);
                }
                Event::Impact { projectile, at, .. } => {
                    let splash = self.splash.remove(&projectile).unwrap_or(0);
                    let size = 0.25 + splash as f32 / sim3d::space::SUB as f32;
                    self.start(Kind::Burst { size }, to_view(at), born, projectile);
                }
                Event::Destroyed { unit, at, .. } => self.start(Kind::Blast { size: 0.6 }, to_view(at), born, unit),
                Event::Wrecked { wreck, .. } => {
                    let Some(w) = world.wrecks().iter().find(|w| w.id == wreck) else { continue };
                    let structure = world.types()[w.kind].structure;
                    let size = structure.map_or(0.5, |s| 0.35 * s.width.max(s.depth) as f32);
                    if structure.is_some() {
                        // A building goes up in a bigger blast than the unit's own, and the ground shakes.
                        self.start(Kind::Blast { size: size * 1.4 }, to_view(w.pos), born, wreck);
                        self.quakes.push((to_view(w.pos), born, size));
                    }
                    let until = born + SMOKE_TICKS[usize::from(structure.is_some())];
                    let building = structure.is_some();
                    self.smoke.push(Smoke { wreck, at: to_view(w.pos), born, until, size, building });
                }
                _ => {}
            }
        }
        if self.live.len() > MAX_LIVE {
            self.live.drain(..self.live.len() - MAX_LIVE);
        }
        // Shots that flew off the map never land; forget them after a while.
        if self.splash.len() > MAX_LIVE {
            let drop: Vec<u32> = self.splash.keys().take(self.splash.len() - MAX_LIVE).copied().collect();
            for k in drop {
                self.splash.remove(&k);
            }
        }
    }

    fn start(&mut self, kind: Kind, at: V3, born: f32, seed: u32) {
        self.live.push(Effect { kind, at, born, seed });
    }

    /// How far the view shakes at `now` when it looks at `focus`, in cells: from each building that went up lately,
    /// strongest at first and close by, dying away over `SHAKE_TICKS` and `SHAKE_REACH`.
    pub fn shake(&mut self, now: f32, focus: V3) -> f32 {
        self.quakes.retain(|q| now - q.1 < SHAKE_TICKS);
        self.quakes
            .iter()
            .filter(|q| now >= q.1)
            .map(|&(at, born, size)| {
                let d = ((at[0] - focus[0]).powi(2) + (at[1] - focus[1]).powi(2)).sqrt();
                let fade = (1.0 - (now - born) / SHAKE_TICKS).powi(2);
                0.12 * size * fade * (1.0 - d / SHAKE_REACH).max(0.0)
            })
            .sum::<f32>()
            .min(0.4)
    }

    /// How many effects and smoking wrecks are playing.
    pub fn len(&self) -> usize {
        self.live.len() + self.smoke.len() + self.flights.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The blobs to draw at time `now` (in ticks, fractions between them), farthest from `eye` first so smoke
    /// covers properly. Finished effects, and smoke from wrecks that have gone, are dropped.
    pub fn puffs(&mut self, world: &World, now: f32, eye: V3) -> Vec<Puff> {
        self.live.retain(|e| now - e.born < life(e.kind));
        self.smoke.retain(|s| now < s.until + PUFF_LIFE && world.wrecks().iter().any(|w| w.id == s.wreck));
        let mut out = Vec::new();
        for e in &self.live {
            let age = now - e.born;
            if age < 0.0 {
                continue;
            }
            draw(e, age / life(e.kind), &mut out);
        }
        for s in &self.smoke {
            smoke(s, now, &mut out);
        }
        self.flights.puffs(now, &mut out);
        let d2 = |p: &Puff| (0..3).map(|i| (p.at[i] - eye[i]).powi(2)).sum::<f32>();
        out.sort_by(|a, b| d2(b).total_cmp(&d2(a)));
        out
    }
}

fn life(kind: Kind) -> f32 {
    match kind {
        Kind::Flash => FLASH_TICKS,
        Kind::Burst { .. } => BURST_TICKS,
        Kind::Blast { .. } => BLAST_TICKS,
    }
}

/// A number from 0 to 1 from two others, the same every time.
fn hash(a: u32, b: u32) -> f32 {
    let mut x = a.wrapping_mul(0x9e37_79b9) ^ b.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65536.0
}

fn alpha(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0) as u8
}

/// One effect `t` of the way through its life.
fn draw(e: &Effect, t: f32, out: &mut Vec<Puff>) {
    let [x, y, z] = e.at;
    match e.kind {
        Kind::Flash => out.push(Puff::round(e.at, 0.22 * (1.0 - 0.5 * t), [255, 235, 170, alpha(1.0 - t)], 1.0)),
        Kind::Burst { size } => {
            // Lifted by most of its radius, so the ground doesn't cut its glow off in a hard line.
            let radius = size * (0.4 + 0.8 * t);
            out.push(Puff::round([x, y, z + 0.8 * radius], radius, [255, 160, 60, alpha(1.0 - t)], 1.0));
            let dust = [x, y, z + 0.3 * size * t];
            out.push(Puff::round(dust, size * (0.6 + 1.2 * t), [125, 110, 95, alpha(0.55 * (1.0 - t))], 0.0));
            // Sparks thrown out and falling back, each a short streak pointing back along its path.
            for k in 0..SPARKS {
                let turn = std::f32::consts::TAU * (k as f32 + hash(e.seed, k + 31)) / SPARKS as f32;
                let reach = 0.3 + 0.4 * size;
                let (out_by, up_by) = ((0.5 + hash(e.seed, k + 37)) * reach, (0.4 + hash(e.seed, k + 41)) * reach);
                let spark = |t: f32| {
                    [x + turn.cos() * out_by * t, y + turn.sin() * out_by * t, z + up_by * 4.0 * t * (1.0 - t)]
                };
                let (head, tail) = (spark(t), spark((t - 0.06).max(0.0)));
                let back = [tail[0] - head[0], tail[1] - head[1], tail[2] - head[2]];
                out.push(Puff::streak(head, 0.02, [255, 215, 140, alpha(1.2 * (1.0 - t))], 1.0, back));
            }
        }
        Kind::Blast { size } => {
            for k in 0..5u32 {
                let (dx, dy) = (hash(e.seed, k) - 0.5, hash(e.seed, k + 7) - 0.5);
                let spread = size * (0.3 + t);
                let at = [x + dx * spread, y + dy * spread, z + size * (0.2 + 0.6 * t * hash(e.seed, k + 13))];
                out.push(Puff::round(at, size * (0.35 + 0.7 * t), [255, 125, 45, alpha(1.0 - t)], 1.0));
            }
            let at = [x, y, z + size * (0.3 + t)];
            out.push(Puff::round(at, size * (0.8 + 1.4 * t), [55, 50, 46, alpha(0.7 * (1.0 - t))], 0.0));
        }
    }
}

/// The puffs rising from a smoking wreck at `now`: one born every few ticks, each rising, spreading and fading.
fn smoke(s: &Smoke, now: f32, out: &mut Vec<Puff>) {
    let first = ((now - PUFF_LIFE - s.born) / PUFF_EVERY).ceil().max(0.0) as u32;
    let last = ((now.min(s.until) - s.born) / PUFF_EVERY).floor().max(-1.0);
    if last < 0.0 {
        return;
    }
    // Thinner as the wreck stops burning.
    let left = ((s.until - now) / (s.until - s.born)).clamp(0.0, 1.0);
    for k in first..=last as u32 {
        let age = now - (s.born + k as f32 * PUFF_EVERY);
        if !(0.0..PUFF_LIFE).contains(&age) {
            continue;
        }
        let u = age / PUFF_LIFE;
        let drift = 0.8 * s.size * u;
        let at = [
            s.at[0] + (hash(s.wreck, k) - 0.5) * s.size * 0.6 + drift,
            s.at[1] + (hash(s.wreck, k + 101) - 0.5) * s.size * 0.6,
            s.at[2] + s.size * (0.2 + 3.0 * u),
        ];
        let a = 0.6 * (1.0 - u) * (0.3 + 0.7 * left) * (u * 8.0).min(1.0);
        out.push(Puff::round(at, s.size * (0.3 + 0.9 * u), [72, 70, 68, alpha(a)], 0.0));
    }
    // A fallen building burns at its foot for the first third of its smoke.
    if s.building && left > 0.66 {
        let flicker = 0.75 + 0.25 * hash(s.wreck, now as u32);
        let at = [s.at[0], s.at[1], s.at[2] + 0.15 * s.size];
        out.push(Puff::round(at, 0.45 * s.size * flicker, [255, 120, 40, alpha(0.7 * flicker)], 1.0));
    }
}
