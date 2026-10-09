//! Sound from the world's `events`: a crack for a shot, a boom for a shell, a thud where it lands, a blast when a
//! unit is destroyed (a bigger one with a long low rumble for a building), and for your own side a chime when something is finished and
//! a clunk when a builder places a frame. Played through the platform's mixer, louder near the middle of the view
//! and panned to where on screen it happened. The clips are made in code, generic placeholders until a setting
//! pack brings its own. Like the effects they only read: nothing in `events` feeds back into the state.

use rts_platform::audio::{Bus, ClipId, Mixer, Sound};
use rts_platform::wav::Clip;
use sim3d::world::{Event, World};
use view3d::camera::Camera;
use view3d::maths::V3;
use view3d::to_view;

use crate::control::Screen;

/// The kinds of sound, in the order their clips are made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cue {
    Shot,
    Shell,
    Hit,
    Blast,
    BigBlast,
    Built,
    Placed,
}

pub const CUES: [Cue; 7] = [Cue::Shot, Cue::Shell, Cue::Hit, Cue::Blast, Cue::BigBlast, Cue::Built, Cue::Placed];

/// The clips' sample rate.
pub const RATE: u32 = 22_050;

/// The clips in the mixer, by cue.
#[derive(Clone, Debug)]
pub struct Sounds {
    clips: Vec<ClipId>,
}

impl Sounds {
    /// Make every cue's clip and add it to `mixer`.
    pub fn new(mixer: &mut Mixer) -> Sounds {
        Sounds { clips: CUES.iter().map(|&c| mixer.add_clip(clip(c))).collect() }
    }

    /// What to play for the events of a tick, seen by `camera` on a `scene`-sized view, for `player`'s side.
    pub fn for_events(
        &self,
        world: &World,
        events: &[Event],
        camera: &Camera,
        scene: Screen,
        player: Option<u8>,
    ) -> Vec<Sound> {
        let mut out = Vec::new();
        for e in events {
            let (cue, at) = match *e {
                Event::Fired { unit, from, .. } => {
                    let splash =
                        world.unit(unit).and_then(|u| world.types()[u.kind].weapon.as_ref()).map_or(0, |w| w.splash);
                    (if splash > 0 { Cue::Shell } else { Cue::Shot }, to_view(from))
                }
                Event::Impact { at, .. } => (Cue::Hit, to_view(at)),
                Event::Destroyed { at, .. } => (Cue::Blast, to_view(at)),
                Event::Wrecked { wreck, .. } => {
                    let Some(w) = world.wrecks().iter().find(|w| w.id == wreck) else { continue };
                    if world.types()[w.kind].structure.is_none() {
                        continue;
                    }
                    (Cue::BigBlast, to_view(w.pos))
                }
                Event::Built { unit, .. } | Event::Placed { unit, .. } => {
                    let Some(u) = world.unit(unit).filter(|u| Some(u.owner) == player) else { continue };
                    (if matches!(e, Event::Built { .. }) { Cue::Built } else { Cue::Placed }, to_view(u.pos))
                }
                _ => continue,
            };
            // Under fog of war the player hears only what it can see.
            let heard =
                player.zip(world.vision()).is_none_or(|(p, v)| v.shows(p, at[0].floor() as i32, at[1].floor() as i32));
            if !heard {
                continue;
            }
            if let Some(s) = self.sound(world, cue, at, camera, scene) {
                out.push(s);
            }
        }
        out
    }

    /// One cue at a point: interface cues at full volume in the middle, the rest by where the point is in view.
    pub fn sound(&self, world: &World, cue: Cue, at: V3, camera: &Camera, scene: Screen) -> Option<Sound> {
        let (bus, priority, max_instances, base) = match cue {
            Cue::Shot => (Bus::Sfx, 1, 4, 0.5),
            Cue::Shell => (Bus::Sfx, 2, 3, 0.6),
            Cue::Hit => (Bus::Sfx, 1, 4, 0.45),
            Cue::Blast => (Bus::Sfx, 4, 3, 0.8),
            Cue::BigBlast => (Bus::Sfx, 6, 2, 1.0),
            Cue::Built | Cue::Placed => (Bus::Ui, 8, 2, 0.6),
        };
        let (gain, pan) = if bus == Bus::Ui { (base, 0.0) } else { place(world, at, camera, scene)? };
        let gain = if bus == Bus::Ui { gain } else { base * gain };
        let clip = self.clips[CUES.iter().position(|&c| c == cue).unwrap_or(0)];
        Some(Sound { clip, key: cue as u32, bus, gain, pan, speed: 1.0, priority, max_instances })
    }
}

/// How loud a sound at `at` is (0 to 1) and where it pans, or `None` when it is too far off to hear. Full near the
/// middle of the view, fading over the ground the view covers, and quieter off screen.
pub fn place(world: &World, at: V3, camera: &Camera, scene: Screen) -> Option<(f32, f32)> {
    let span = camera.distance() * 0.9 + 6.0;
    let d = ((at[0] - camera.focus[0]).powi(2) + (at[1] - camera.focus[1]).powi(2)).sqrt();
    let near = (1.0 - d / span).clamp(0.0, 1.0);
    // Pulled back over the whole map everything is far away, and quieter.
    let zoom = 1.0 - 0.5 * camera.zoom;
    let seen = camera.project(world.map(), at, scene.width, scene.height);
    let (on, pan) = match seen {
        Some((x, y)) if (0.0..=scene.width).contains(&x) && (0.0..=scene.height).contains(&y) => {
            (1.0, (x / scene.width * 2.0 - 1.0) * 0.8)
        }
        Some((x, _)) => (0.4, if x < 0.0 { -0.8 } else { 0.8 }),
        None => (0.4, 0.0),
    };
    let gain = near * near * zoom * on;
    (gain > 0.02).then_some((gain, pan.clamp(-1.0, 1.0)))
}

/// A cue's clip, made in code: shaped noise and low tones, the same every time.
pub fn clip(cue: Cue) -> Clip {
    let (seconds, seed) = match cue {
        Cue::Shot => (0.2, 1),
        Cue::Shell => (0.6, 2),
        Cue::Hit => (0.3, 3),
        Cue::Blast => (1.2, 4),
        Cue::BigBlast => (2.8, 5),
        Cue::Built => (0.45, 6),
        Cue::Placed => (0.2, 7),
    };
    let n = (seconds * RATE as f32) as usize;
    let mut noise = Noise(seed);
    let mut low = 0.0f32;
    // A much darker filter, two poles deep, for a building's long rumble.
    let (mut dark, mut deep) = (0.0f32, 0.0f32);
    let mut samples = Vec::with_capacity(n);
    let tau = std::f32::consts::TAU;
    for i in 0..n {
        let t = i as f32 / RATE as f32;
        let w = noise.next();
        // A one-pole low-pass over the noise, darker for the bigger sounds.
        let k = match cue {
            Cue::Shot | Cue::Hit => 0.35,
            Cue::Shell | Cue::Placed => 0.15,
            _ => 0.06,
        };
        low += k * (w - low);
        dark += 0.006 * (w - dark);
        deep += 0.006 * (dark - deep);
        let s = match cue {
            Cue::Shot => 0.9 * w * (-t * 40.0).exp() + 0.6 * (tau * 120.0 * t).sin() * (-t * 25.0).exp(),
            Cue::Shell => 0.8 * low * (-t * 7.0).exp() + 0.8 * (tau * 55.0 * t).sin() * (-t * 9.0).exp(),
            Cue::Hit => 1.4 * low * (-t * 18.0).exp() + 0.3 * w * (-t * 60.0).exp(),
            Cue::Blast => {
                3.0 * low * (-t * 3.5).exp() * (1.0 - (-t * 60.0).exp())
                    + 0.6 * (tau * 42.0 * t).sin() * (-t * 4.0).exp()
            }
            Cue::BigBlast => {
                // The blast, then a deep rumble that swells and rolls on under it: filtered noise and a low tone
                // that wavers.
                let rumble = (1.0 - (-t * 6.0).exp()) * (-t * 1.1).exp();
                let waver = 1.0 + 0.3 * (tau * 3.0 * t).sin();
                // Rounded off rather than clipped where the two add up.
                (3.4 * low * (-t * 3.5).exp() * (1.0 - (-t * 40.0).exp())
                    + rumble * (30.0 * deep + 0.6 * (tau * 28.0 * t).sin() * waver))
                    .tanh()
            }
            Cue::Built => {
                let f = if t < 0.15 { 660.0 } else { 880.0 };
                let start = if t < 0.15 { 0.0 } else { 0.15 };
                0.5 * (tau * f * t).sin() * (-(t - start) * 9.0).exp() * (1.0 - (-(t - start) * 400.0).exp())
            }
            Cue::Placed => 0.7 * (tau * 170.0 * t).sin() * (-t * 30.0).exp() + 0.5 * low * (-t * 35.0).exp(),
        };
        samples.push(s.clamp(-1.0, 1.0));
    }
    Clip { rate: RATE, samples }
}

/// White noise from a seed: a small xorshift, the same every run.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        let mut x = self.0.wrapping_mul(0x9e37_79b9).wrapping_add(0x7f4a_7c15) | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}
