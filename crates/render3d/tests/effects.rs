//! Effects from events: flashes, bursts, blasts and smoke, worked out with no GPU, and drawn once on one.

use ai3d::skirmish::{self, ARTILLERY, BUILDER, EXTRACTOR, TANK};
use render3d::Renderer;
use render3d::effects::{BURST_TICKS, Effects, FLASH_TICKS, Puff, SHAKE_TICKS, SMOKE_TICKS};
use render3d::shapes::Shapes;
use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, World};
use view3d::camera::Camera;

const EYE: [f32; 3] = [12.0, 30.0, 20.0];

fn field() -> World {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.set_store(0, vec![1_000_000; 2], vec![1_000_000; 2]);
    world
}

/// Step until `f` picks out an event, feeding every event to the effects.
fn until<T>(world: &mut World, fx: &mut Effects, ticks: u32, mut f: impl FnMut(&Event) -> Option<T>) -> T {
    for _ in 0..ticks {
        let events = world.step();
        fx.observe(world, &events);
        if let Some(t) = events.iter().find_map(&mut f) {
            return t;
        }
    }
    panic!("the event never came");
}

fn glows(puffs: &[Puff]) -> Vec<&Puff> {
    puffs.iter().filter(|p| p.glow > 0.5 && p.colour[3] > 0).collect()
}

#[test]
fn a_shot_flashes_at_the_muzzle_and_bursts_where_it_lands_bigger_for_a_shell() {
    let mut world = field();
    let tank = world.spawn(TANK, 6 * SUB, 8 * SUB);
    let gun = world.spawn(ARTILLERY, 6 * SUB, 12 * SUB);
    let target = world.spawn_for(1, BUILDER, 10 * SUB, 8 * SUB);
    let far = world.spawn_for(1, BUILDER, 14 * SUB, 12 * SUB);
    world.command(Command::Attack { unit: tank, target });
    world.command(Command::Attack { unit: gun, target: far });
    let mut fx = Effects::default();
    let from = until(&mut world, &mut fx, 200, |e| match e {
        Event::Fired { unit, from, .. } if *unit == tank => Some(view3d::to_view(*from)),
        _ => None,
    });
    let now = world.tick() as f32 - 1.0;
    let flash = fx.puffs(&world, now, EYE);
    assert!(glows(&flash).iter().any(|p| (p.at[0] - from[0]).abs() < 0.01 && (p.at[1] - from[1]).abs() < 0.01));
    assert!(
        fx.puffs(&world, now + FLASH_TICKS + 0.5, EYE)
            .iter()
            .all(|p| (p.at[0] - from[0]).abs() > 0.01 || p.at[1] != from[1])
    );

    // The bursts: the tank's shot and the artillery's shell, which has splash.
    let mut sizes = std::collections::BTreeMap::new();
    for _ in 0..400 {
        let events = world.step();
        fx.observe(&world, &events);
        for e in &events {
            if let Event::Impact { at, .. } = e {
                let at = view3d::to_view(*at);
                let now = world.tick() as f32 - 1.0 + BURST_TICKS / 2.0;
                let biggest = fx
                    .puffs(&world, now, EYE)
                    .iter()
                    .filter(|p| p.glow > 0.5 && (p.at[0] - at[0]).abs() < 0.01 && (p.at[1] - at[1]).abs() < 0.01)
                    .map(|p| p.radius)
                    .fold(0.0f32, f32::max);
                sizes.insert(at[1] > 10.0, biggest);
            }
        }
        if sizes.len() == 2 {
            break;
        }
    }
    let (shot, shell) = (sizes[&false], sizes[&true]);
    assert!(shot > 0.0 && shell > 1.5 * shot, "a shell's burst ({shell}) is bigger than a shot's ({shot})");
}

#[test]
fn a_wrecked_building_blasts_then_smokes_until_its_time_is_up_or_it_is_cleared() {
    let mut world = field();
    let building = world.spawn_for(1, EXTRACTOR, 10 * SUB + SUB / 2, 8 * SUB + SUB / 2);
    let tank = world.spawn(TANK, 6 * SUB, 8 * SUB);
    world.command(Command::Attack { unit: tank, target: building });
    let mut fx = Effects::default();
    let wreck = until(&mut world, &mut fx, 3000, |e| match e {
        Event::Wrecked { unit, wreck } if *unit == building => Some(*wreck),
        _ => None,
    });
    let born = world.tick() as f32 - 1.0;
    let at = view3d::to_view(world.wrecks().iter().find(|w| w.id == wreck).unwrap().pos);

    // The view shakes, most when looking right at it, and settles.
    let close = fx.shake(born + 1.0, at);
    let off = fx.shake(born + 1.0, [at[0] + 12.0, at[1], 0.0]);
    assert!(close > 0.02 && off > 0.0 && off < close / 1.5, "{close} close, {off} further off");
    assert!(fx.shake(born + SHAKE_TICKS / 2.0, at) < close / 2.0);
    assert_eq!(fx.shake(born + SHAKE_TICKS + 1.0, at), 0.0);
    let near = |p: &Puff| (p.at[0] - at[0]).abs() < 1.5 && (p.at[1] - at[1]).abs() < 1.5;
    assert!(glows(&fx.puffs(&world, born + 2.0, EYE)).iter().filter(|p| near(p)).count() >= 5, "a blast of fire");

    // Later the blast is over and smoke rises: puffs over the wreck, the older ones higher, darker than fire.
    let later = fx.puffs(&world, born + 100.0, EYE);
    let smoke: Vec<&Puff> = later.iter().filter(|p| p.glow < 0.5 && near(p)).collect();
    assert!(smoke.len() >= 8, "{} puffs", smoke.len());
    let top = smoke.iter().map(|p| p.at[2]).fold(0.0f32, f32::max);
    assert!(top > at[2] + 0.6, "the smoke rises");
    assert!(glows(&later).iter().any(|p| near(p)), "and the building burns at its foot");
    // Farthest from the eye first, so smoke covers what is behind it.
    let d = |p: &Puff| (0..3).map(|i| (p.at[i] - EYE[i]).powi(2)).sum::<f32>();
    assert!(later.windows(2).all(|w| d(&w[0]) >= d(&w[1])));

    // Once its time is up it stops.
    assert!(fx.puffs(&world, born + SMOKE_TICKS[1] + 100.0, EYE).is_empty());
    assert!(fx.is_empty());

    // A wreck that is cleared stops smoking at once.
    let mut fx = Effects::default();
    fx.observe(&world, &[Event::Wrecked { unit: building, wreck }]);
    assert!(!fx.puffs(&world, world.tick() as f32 + 20.0, EYE).is_empty());
    let builder = world.spawn(BUILDER, 8 * SUB, 8 * SUB);
    world.command(Command::Reclaim { unit: builder, wreck });
    until(&mut world, &mut fx, 3000, |e| matches!(e, Event::Reclaimed { .. }).then_some(()));
    assert!(fx.puffs(&world, world.tick() as f32, EYE).is_empty());
}

#[test]
fn effects_draw_as_glowing_and_smoky_blobs_over_the_scene() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let world = field();
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.3;
    camera.focus = [12.0, 12.0, 0.0];
    camera.settle(world.map());
    let (w, h) = (480u32, 270u32);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let shapes = Shapes::default().shapes(&world, 1.0);
    let plain = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let fire = Puff { at: [10.0, 12.0, 0.3], radius: 0.6, colour: [255, 140, 50, 255], glow: 1.0 };
    let smoke = Puff { at: [14.0, 12.0, 0.3], radius: 0.6, colour: [40, 40, 40, 200], glow: 0.0 };
    renderer.set_effects(&[fire, smoke]);
    let image = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let px = |img: &[u8], p: [f32; 3]| {
        let (x, y) = camera.project(world.map(), p, w as f32, h as f32).unwrap();
        let i = ((y as u32) * w + x as u32) as usize * 4;
        [img[i] as i32, img[i + 1] as i32, img[i + 2] as i32]
    };
    let (before, after) = (px(&plain, fire.at), px(&image, fire.at));
    assert!(after[0] > before[0] + 80 && after[0] > after[2] + 80, "fire brightens the ground: {before:?} {after:?}");
    let (before, after) = (px(&plain, smoke.at), px(&image, smoke.at));
    assert!(after.iter().sum::<i32>() < before.iter().sum::<i32>() - 40, "smoke darkens it: {before:?} {after:?}");
    // A glow is hidden behind what is nearer: put the fire under the ground and it is gone.
    renderer.set_effects(&[Puff { at: [10.0, 12.0, -2.0], radius: 0.6, ..fire }]);
    let hidden = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    assert_eq!(px(&hidden, [10.0, 12.0, 0.0]), px(&plain, [10.0, 12.0, 0.0]));
}
