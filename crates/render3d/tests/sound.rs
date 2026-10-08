//! Sound from events, with no sound card: the clips, which events make which sound, and how loud and where.

use ai3d::skirmish::{self, ARTILLERY, BUILDER, GENERATOR, TANK};
use render3d::control::Screen;
use render3d::sound::{CUES, Cue, Sounds, clip, place};
use rts_platform::audio::{Bus, Mixer};
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, World};
use view3d::camera::Camera;

const SCENE: Screen = Screen { width: 800.0, height: 600.0 };

fn camera_over(world: &World, x: f32, y: f32) -> Camera {
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.3;
    camera.focus = [x, y, 0.0];
    camera.settle(world.map());
    camera
}

#[test]
fn every_cue_has_its_own_clip_made_the_same_each_time() {
    let mut seen = Vec::new();
    for cue in CUES {
        let c = clip(cue);
        let peak = c.samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.1 && peak <= 1.0, "{cue:?} peaks at {peak}");
        assert!(c.seconds() > 0.1 && c.seconds() < 3.0, "{cue:?} lasts {}", c.seconds());
        assert_eq!(c, clip(cue), "{cue:?} is the same every time");
        assert!(!seen.contains(&c.samples), "{cue:?} differs from the others");
        seen.push(c.samples);
    }
    // A blast is longer and lower than a shot.
    assert!(clip(Cue::BigBlast).seconds() > clip(Cue::Blast).seconds());
    assert!(clip(Cue::Blast).seconds() > 4.0 * clip(Cue::Shot).seconds());
}

#[test]
fn a_sound_is_loudest_in_the_middle_of_the_view_and_pans_to_its_side() {
    let world = World::new(Heightmap::flat(64, 64, 0), skirmish::types(), 1);
    let camera = camera_over(&world, 32.0, 32.0);
    let (middle, _) = place(&world, [32.0, 32.0, 0.0], &camera, SCENE).unwrap();
    let (aside, _) = place(&world, [36.0, 32.0, 0.0], &camera, SCENE).unwrap();
    assert!(middle > aside, "{middle} > {aside}");
    let (_, left) = place(&world, [29.0, 32.0, 0.0], &camera, SCENE).unwrap();
    let (_, right) = place(&world, [35.0, 32.0, 0.0], &camera, SCENE).unwrap();
    assert!(left < -0.1 && right > 0.1, "pans {left} and {right}");
    assert_eq!(place(&world, [2.0, 2.0, 0.0], &camera, SCENE), None, "far across the map it is silent");
    // Pulled right back, the whole map is heard, more quietly.
    let mut far = camera.clone();
    far.zoom = 1.0;
    let (gain, _) = place(&world, [32.0, 32.0, 0.0], &far, SCENE).unwrap();
    assert!(gain < middle && gain > 0.2);
}

#[test]
fn fights_make_shots_hits_and_blasts_and_only_your_own_building_chimes() {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.set_store(0, vec![1_000_000; 2], vec![1_000_000; 2]);
    world.set_store(1, vec![1_000_000; 2], vec![1_000_000; 2]);
    let tank = world.spawn(TANK, 6 * SUB, 8 * SUB);
    let gun = world.spawn(ARTILLERY, 6 * SUB, 10 * SUB);
    let target = world.spawn_for(1, BUILDER, 10 * SUB, 9 * SUB);
    let mine = world.spawn(BUILDER, 4 * SUB, 16 * SUB);
    let theirs = world.spawn_for(1, BUILDER, 18 * SUB, 16 * SUB);
    world.command(Command::Attack { unit: tank, target });
    world.command(Command::Attack { unit: gun, target });
    world.command(Command::Build { unit: mine, kind: GENERATOR, cx: 6, cy: 17 });
    world.command(Command::Build { unit: theirs, kind: GENERATOR, cx: 16, cy: 17 });
    let mut mixer = Mixer::new(22_050);
    let sounds = Sounds::new(&mut mixer);
    let camera = camera_over(&world, 11.0, 12.0);
    let mut heard = std::collections::BTreeMap::<u32, (usize, Bus)>::new();
    let mut placed = Vec::new();
    for _ in 0..4000 {
        let events = world.step();
        for s in sounds.for_events(&world, &events, &camera, SCENE, Some(0)) {
            heard.entry(s.key).or_insert((0, s.bus)).0 += 1;
            mixer.play(s);
        }
        placed.extend(events.iter().filter_map(|e| match e {
            Event::Placed { unit, .. } => Some(world.unit(*unit).unwrap().owner),
            _ => None,
        }));
    }
    for cue in [Cue::Shot, Cue::Shell, Cue::Hit, Cue::Blast, Cue::Placed, Cue::Built] {
        assert!(heard.contains_key(&(cue as u32)), "{cue:?} was heard: {heard:?}");
    }
    placed.sort();
    assert_eq!(placed, [0, 1], "both sides placed a frame");
    assert_eq!(heard[&(Cue::Placed as u32)], (1, Bus::Ui), "but only ours was heard");
    assert_eq!(heard[&(Cue::Built as u32)].1, Bus::Ui);
    assert_eq!(heard[&(Cue::Shot as u32)].1, Bus::Sfx);

    // And the mixer makes sound from them.
    let mut mixer = Mixer::new(22_050);
    let sounds = Sounds::new(&mut mixer);
    mixer.play(sounds.sound(&world, Cue::Blast, [11.0, 12.0, 0.0], &camera, SCENE).unwrap());
    let mut out = vec![0.0; 2 * 4410];
    mixer.render(&mut out, 2);
    assert!(out.iter().any(|s| s.abs() > 0.05));
}

#[test]
fn a_building_going_up_rumbles_on_after_the_blast() {
    let rms = |c: &rts_platform::wav::Clip, from: f32, to: f32| {
        let s = &c.samples[(from * c.rate as f32) as usize..(to * c.rate as f32) as usize];
        (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
    };
    let (building, vehicle) = (clip(Cue::BigBlast), clip(Cue::Blast));
    let tail = rms(&building, 1.2, 2.4);
    assert!(tail > 0.08, "the rumble carries on: {tail}");
    // Low: it crosses zero far less often than the blast's crack does.
    let crossings = |c: &rts_platform::wav::Clip, from: f32, to: f32| {
        let s = &c.samples[(from * c.rate as f32) as usize..(to * c.rate as f32) as usize];
        s.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f32 / (to - from)
    };
    assert!(crossings(&building, 1.2, 2.4) < 120.0, "{} a second", crossings(&building, 1.2, 2.4));
    assert!(tail > 4.0 * rms(&vehicle, 1.0, 1.15), "a vehicle's blast has no such tail");
}
