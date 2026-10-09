//! Shots in flight, drawn as what they are: a tracer is a bright streak, a shell a glowing round trailing a little
//! smoke, a missile a dark body with a flame at its tail and a thick trail of smoke that hangs in the air after it
//! has landed. Which look a unit kind's shots have comes from the setting (`shots.json`); a kind it leaves out gets
//! a shell if its weapon lobs and a tracer if not.
//!
//! A projectile's flight is a closed form of the time since it was fired (`sim3d::weapon`), so its path is worked
//! out here again in floats at any fraction of a tick: the head moves smoothly and the trail lies exactly along the
//! path it flew. Like the other effects these only read the world's events and projectiles; nothing feeds back.
//! Tracers, smoking shells and rocket trails are the genre's usual look; the code and numbers are ours.

use std::collections::BTreeMap;

use sim3d::space::SUB;
use sim3d::weapon::Projectile;
use sim3d::world::{Event, World};
use view3d::maths::V3;
use view3d::{HEIGHT_PER_CELL, to_view};

use crate::effects::Puff;
use crate::json::{self, Value};

/// How a kind's shots look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Tracer,
    Shell,
    Missile,
}

impl Look {
    fn named(name: &str) -> Option<Look> {
        match name {
            "tracer" => Some(Look::Tracer),
            "shell" => Some(Look::Shell),
            "missile" => Some(Look::Missile),
            _ => None,
        }
    }
}

/// The look of each unit kind's shots, by kind index, from the setting.
#[derive(Clone, Debug, Default)]
pub struct Looks {
    by_kind: Vec<Option<Look>>,
}

impl Looks {
    /// The looks for the generic skirmish (`ai3d::skirmish`), from `assets/skirmish/shots.json`.
    pub fn skirmish() -> Looks {
        Looks::from_list(include_str!("../../../assets/skirmish/shots.json"), &ai3d::skirmish::KINDS)
            .expect("the skirmish's shot looks load")
    }

    /// Looks from a list (`shots.json`: `kinds` naming a look for each kind it sets), for the kinds named in `kinds`
    /// by index.
    pub fn from_list(list: &str, kinds: &[&str]) -> Result<Looks, String> {
        let doc = json::parse(list)?;
        let named = doc.get("kinds").ok_or("shots: no kinds")?;
        for (name, look) in named.fields() {
            if !kinds.contains(&name.as_str()) {
                return Err(format!("shots: no unit kind called {name}"));
            }
            if look.str().and_then(Look::named).is_none() {
                return Err(format!("shots: {name} has no look called {look:?}"));
            }
        }
        let by_kind = kinds.iter().map(|k| named.get(k).and_then(Value::str).and_then(Look::named)).collect();
        Ok(Looks { by_kind })
    }

    /// The look of unit kind `kind`'s shots in `world`.
    pub fn of(&self, world: &World, kind: usize) -> Look {
        self.by_kind.get(kind).copied().flatten().unwrap_or_else(|| {
            let lobs = world.types().get(kind).and_then(|t| t.weapon.as_ref()).is_some_and(|w| w.gravity > 0);
            if lobs { Look::Shell } else { Look::Tracer }
        })
    }
}

/// How long a tracer's streak is, in ticks of its flight.
const STREAK_TICKS: f32 = 1.4;
/// How long a puff of a shell's and a missile's trail lasts, in ticks, and how often one is left, in ticks of flight.
const SHELL_TRAIL: f32 = 18.0;
const SHELL_EVERY: f32 = 1.0;
const MISSILE_EVERY: f32 = 0.5;
const MISSILE_TRAIL: f32 = 40.0;
/// How long the smoke at a missile's launch lasts, in ticks.
const LAUNCH_SMOKE: f32 = 20.0;
/// The most shots followed at once; the oldest go first.
const MAX_FLIGHTS: usize = 2000;

/// One shot, from when it was fired until its trail has gone.
#[derive(Clone, Debug)]
struct Flight {
    shot: Projectile,
    look: Look,
    /// The tick it left the muzzle, on the picture's clock (which runs a tick behind the state).
    fired: f32,
    /// When it stopped, as a time of flight in ticks, once it has.
    ended: Option<f32>,
}

/// The shots being drawn now.
#[derive(Clone, Debug, Default)]
pub struct Flights {
    looks: Looks,
    live: BTreeMap<u32, Flight>,
}

impl Flights {
    pub fn new(looks: Looks) -> Flights {
        Flights { looks, live: BTreeMap::new() }
    }

    /// Follow the shots fired and stopped in the tick just stepped; `world` is the state after it.
    pub fn observe(&mut self, world: &World, events: &[Event]) {
        // The same clock as `Effects`: what happened in this step shows from the last tick on.
        let born = world.tick() as f32 - 1.0;
        for e in events {
            match *e {
                Event::Fired { unit, projectile, from, aim } => {
                    // The flight is worked out from the shot exactly as the simulation launched it.
                    let Some(u) = world.unit(unit) else { continue };
                    let Some(weapon) = &world.types()[u.kind].weapon else { continue };
                    let shot = Projectile::launch(projectile, (unit, u.owner), u.kind, weapon, from, aim);
                    let look = self.looks.of(world, u.kind);
                    self.live.insert(projectile, Flight { shot, look, fired: born, ended: None });
                }
                Event::Impact { projectile, at, .. } => {
                    if let Some(f) = self.live.get_mut(&projectile) {
                        // It struck during its last tick of flight, part of the way along.
                        let k = f.shot.flown as f32;
                        let (a, b, hit) = (place(&f.shot, k), place(&f.shot, k + 1.0), to_view(at));
                        let share = (distance(a, hit) / distance(a, b).max(1e-6)).clamp(0.0, 1.0);
                        f.ended = Some(k + share);
                    }
                }
                _ => {}
            }
        }
        // Keep each shot's count of ticks flown; one gone without striking anything flew off the map or ran out.
        let flying: BTreeMap<u32, i32> = world.projectiles().iter().map(|p| (p.id, p.flown)).collect();
        for (id, f) in &mut self.live {
            match flying.get(id) {
                Some(&flown) => f.shot.flown = flown,
                None if f.ended.is_none() => f.ended = Some(f.shot.flown as f32 + 1.0),
                None => {}
            }
        }
        if self.live.len() > MAX_FLIGHTS {
            let drop: Vec<u32> = self.live.keys().take(self.live.len() - MAX_FLIGHTS).copied().collect();
            for k in drop {
                self.live.remove(&k);
            }
        }
    }

    /// How many shots are being drawn, in flight or leaving a trail.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The blobs and streaks for every shot at time `now` (in ticks, fractions between them), added to `out`.
    /// Shots whose trails have gone are dropped.
    pub fn puffs(&mut self, now: f32, out: &mut Vec<Puff>) {
        self.live.retain(|_, f| f.ended.is_none_or(|end| now - f.fired - end < linger(f.look)));
        for f in self.live.values() {
            let age = now - f.fired;
            if age < 0.0 {
                continue;
            }
            // The head is where the shot is now, or where it stopped.
            let head = f.ended.map_or(age, |end| age.min(end));
            let flying = f.ended.is_none_or(|end| age < end);
            draw(f, head, age, flying, out);
        }
    }
}

/// How long after a shot stops its trail is still drawn, in ticks.
fn linger(look: Look) -> f32 {
    match look {
        Look::Tracer => 0.0,
        Look::Shell => SHELL_TRAIL,
        Look::Missile => MISSILE_TRAIL,
    }
}

/// Where a shot is after `t` ticks of flight, in view space: the closed form of `Projectile::at` in floats.
pub fn place(p: &Projectile, t: f32) -> V3 {
    let share = t / p.flight.max(1) as f32;
    let along = |a: i32, b: i32| a as f32 + (b - a) as f32 * share;
    let z = p.from.z as f32 + p.climb as f32 * t - p.gravity as f32 * t * (t + 1.0) / 2.0;
    let sub = SUB as f32;
    [along(p.from.x, p.aim.x) / sub, along(p.from.y, p.aim.y) / sub, z / HEIGHT_PER_CELL]
}

fn distance(a: V3, b: V3) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

fn minus(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn alpha(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0) as u8
}

/// One shot whose head is `head` ticks into its flight, `age` ticks after it was fired.
fn draw(f: &Flight, head: f32, age: f32, flying: bool, out: &mut Vec<Puff>) {
    let p = &f.shot;
    let at = place(p, head);
    match f.look {
        Look::Tracer => {
            if flying {
                let tail = place(p, (head - STREAK_TICKS).max(0.0));
                out.push(Puff::streak(at, 0.045, [255, 226, 150, 255], 1.0, minus(tail, at)));
                out.push(Puff::round(at, 0.08, [255, 245, 210, 170], 1.0));
            }
        }
        Look::Shell => {
            trail(f, head, age, (SHELL_EVERY, SHELL_TRAIL), out, |u| {
                (0.07 + 0.12 * u, [150, 145, 138, alpha(0.32 * (1.0 - u))], 0.08 * u)
            });
            if flying {
                let tail = place(p, (head - 0.5).max(0.0));
                out.push(Puff::streak(at, 0.06, [255, 190, 110, 255], 1.0, minus(tail, at)));
            }
        }
        Look::Missile => {
            // Smoke billows round the launcher as the missile leaves.
            if age < LAUNCH_SMOKE {
                let u = age / LAUNCH_SMOKE;
                let from = place(p, 0.0);
                let at = [from[0], from[1], from[2] + 0.15 * u];
                out.push(Puff::round(at, 0.2 + 0.35 * u, [200, 196, 190, alpha(0.7 * (1.0 - u))], 0.0));
            }
            trail(f, head, age, (MISSILE_EVERY, MISSILE_TRAIL), out, |u| {
                let thick = (u * 6.0).min(1.0);
                (0.1 + 0.2 * u, [232, 230, 225, alpha(0.7 * (1.0 - u) * thick)], 0.12 * u)
            });
            if flying {
                let tail = place(p, (head - 1.2).max(0.0));
                let flame = place(p, (head - 1.4).max(0.0));
                let flicker = 0.8 + 0.2 * ((age * 7.3).sin() * 0.5 + 0.5);
                // A wide orange glow round the exhaust and a white-hot core, so the missile reads from far off.
                out.push(Puff::round(flame, 0.26 * flicker, [255, 140, 50, 200], 1.0));
                out.push(Puff::round(flame, 0.1 * flicker, [255, 245, 215, 255], 1.0));
                out.push(Puff::streak(at, 0.07, [60, 60, 64, 255], 0.0, minus(tail, at)));
            }
        }
    }
}

/// A trail of smoke left every `every` ticks of flight up to `head`, each puff lasting `life` ticks; `shape` gives a
/// puff `u` of the way through its life its radius, colour and how far it has risen, in cells.
fn trail(
    f: &Flight,
    head: f32,
    age: f32,
    (every, life): (f32, f32),
    out: &mut Vec<Puff>,
    shape: impl Fn(f32) -> (f32, [u8; 4], f32),
) {
    let first = ((age - life).max(0.0) / every).ceil() as u32;
    let last = (head.max(0.0) / every).floor() as u32;
    for k in first..=last {
        let t = k as f32 * every;
        let u = (age - t) / life;
        if !(0.0..1.0).contains(&u) {
            continue;
        }
        let (radius, colour, rise) = shape(u);
        let [x, y, z] = place(&f.shot, t);
        out.push(Puff::round([x, y, z + rise], radius, colour, 0.0));
    }
}
