//! Shots in flight: each kind's look from the setting, the head where the simulation has the shot, trails along the
//! path it flew that outlast it for a while, and streaks drawn on the GPU.

use ai3d::skirmish::{self, ARTILLERY, BUILDER, TANK};
use render3d::Renderer;
use render3d::effects::{Effects, Puff};
use render3d::shapes::Shapes;
use render3d::shots::{Flights, Look, Looks, place};
use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, World};
use view3d::camera::Camera;
use view3d::to_view;

fn field() -> World {
    World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1)
}

fn near(a: [f32; 3], b: [f32; 3], by: f32) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= by)
}

/// Step until `f` picks out an event, feeding every event to the shots.
fn until<T>(world: &mut World, shots: &mut Flights, ticks: u32, mut f: impl FnMut(&Event) -> Option<T>) -> T {
    for _ in 0..ticks {
        let events = world.step();
        shots.observe(world, &events);
        if let Some(t) = events.iter().find_map(&mut f) {
            return t;
        }
    }
    panic!("the event never came");
}

#[test]
fn the_skirmish_names_its_looks_and_other_kinds_go_by_their_weapon() {
    let world = field();
    let looks = Looks::skirmish();
    assert_eq!(looks.of(&world, TANK), Look::Tracer);
    assert_eq!(looks.of(&world, ARTILLERY), Look::Missile);
    // With nothing named, a lobbing weapon fires shells and a direct one tracers.
    let plain = Looks::default();
    assert_eq!(plain.of(&world, TANK), Look::Tracer);
    assert_eq!(plain.of(&world, ARTILLERY), Look::Shell);
    let kinds = ["tank", "artillery"];
    let shells = Looks::from_list(r#"{"kinds": {"tank": "shell"}}"#, &kinds).unwrap();
    assert_eq!(shells.of(&world, 0), Look::Shell);
    assert!(Looks::from_list(r#"{"kinds": {"boat": "shell"}}"#, &kinds).is_err(), "no such kind");
    assert!(Looks::from_list(r#"{"kinds": {"tank": "laser"}}"#, &kinds).is_err(), "no such look");
}

#[test]
fn the_path_in_floats_follows_the_simulations_shot() {
    let mut world = field();
    world.spawn(ARTILLERY, 4 * SUB, 4 * SUB);
    world.spawn_for(1, BUILDER, 12 * SUB, 6 * SUB);
    let mut checked = 0;
    for _ in 0..400 {
        world.step();
        for p in world.projectiles() {
            for t in 0..=p.flown {
                // The simulation rounds each step down to whole sub-cell units; the float path doesn't.
                assert!(near(place(p, t as f32), to_view(p.at(t)), 0.01), "tick {t} of shot {}", p.id);
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "the artillery fired");
}

#[test]
fn a_tracer_is_a_streak_at_the_shot_and_goes_when_it_lands() {
    let mut world = field();
    let tank = world.spawn(TANK, 4 * SUB, 4 * SUB);
    let target = world.spawn_for(1, BUILDER, 8 * SUB, 4 * SUB);
    world.command(Command::Attack { unit: tank, target });
    let mut shots = Flights::new(Looks::skirmish());
    let id = until(&mut world, &mut shots, 200, |e| match e {
        Event::Fired { projectile, .. } => Some(*projectile),
        _ => None,
    });
    // Each tick in flight, the head is where the shapes used to put the shot: between its last two positions.
    let mut seen = 0;
    while let Some(p) = world.projectiles().iter().find(|p| p.id == id).cloned() {
        for alpha in [0.0, 0.5, 1.0] {
            let now = world.tick() as f32 - 1.0 + alpha;
            let mut puffs = Vec::new();
            shots.puffs(now, &mut puffs);
            let (a, b) = (to_view(p.at((p.flown - 1).max(0))), to_view(p.at(p.flown)));
            let expected = [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * alpha);
            // The streak is the thin one; it has no length yet as the shot leaves the muzzle.
            let streak = puffs.iter().find(|q| q.radius < 0.05).expect("a streak");
            assert!(near(streak.at, expected, 0.01), "{:?} {expected:?}", streak.at);
            // It trails back towards the muzzle.
            assert!(streak.glow == 1.0 && streak.stretch[0] <= 0.0);
            assert_eq!(streak.stretch[0] < 0.0, now > world.tick() as f32 - p.flown as f32, "at {now}");
            seen += 1;
        }
        let events = world.step();
        shots.observe(&world, &events);
    }
    assert!(seen >= 3);
    let mut puffs = Vec::new();
    shots.puffs(world.tick() as f32 + 1.0, &mut puffs);
    assert!(puffs.is_empty() && shots.is_empty(), "a tracer leaves nothing behind");
}

#[test]
fn a_missile_leaves_a_trail_along_its_path_that_hangs_after_it_lands() {
    let mut world = field();
    let gun = world.spawn(ARTILLERY, 4 * SUB, 4 * SUB);
    let target = world.spawn_for(1, BUILDER, 12 * SUB, 8 * SUB);
    world.command(Command::Attack { unit: gun, target });
    let mut shots = Flights::new(Looks::skirmish());
    let (from, aim) = until(&mut world, &mut shots, 200, |e| match e {
        Event::Fired { from, aim, .. } => Some((to_view(*from), to_view(*aim))),
        _ => None,
    });
    until(&mut world, &mut shots, 400, |e| matches!(e, Event::Impact { .. }).then_some(()));
    let now = world.tick() as f32 - 1.0;
    let mut puffs = Vec::new();
    shots.puffs(now + 2.0, &mut puffs);
    let smoke: Vec<&Puff> = puffs.iter().filter(|p| p.glow == 0.0 && p.stretch == [0.0; 3]).collect();
    assert!(smoke.len() > 10, "a trail of smoke: {}", smoke.len());
    // Every puff of it lies over the line from the muzzle to the aim point.
    let (dx, dy) = (aim[0] - from[0], aim[1] - from[1]);
    for p in &smoke {
        let off = ((p.at[0] - from[0]) * dy - (p.at[1] - from[1]) * dx).abs() / (dx * dx + dy * dy).sqrt();
        assert!(off < 0.02, "{off}");
    }
    // No body or flame once it has landed, and the trail thins and goes.
    assert!(puffs.iter().all(|p| p.stretch == [0.0; 3]));
    let mut later = Vec::new();
    shots.puffs(now + 30.0, &mut later);
    let mut thin = later.iter().filter(|p| p.glow == 0.0).count();
    assert!(thin > 0 && thin < smoke.len(), "{thin}");
    for t in [60.0, 200.0] {
        later.clear();
        shots.puffs(now + t, &mut later);
        thin = later.len();
    }
    assert_eq!(thin, 0);
    assert!(shots.is_empty());
}

#[test]
fn effects_include_the_shots() {
    let mut world = field();
    let tank = world.spawn(TANK, 4 * SUB, 4 * SUB);
    let target = world.spawn_for(1, BUILDER, 8 * SUB, 4 * SUB);
    world.command(Command::Attack { unit: tank, target });
    let mut fx = Effects::with_looks(Looks::skirmish());
    for _ in 0..200 {
        let events = world.step();
        fx.observe(&world, &events);
        if !world.projectiles().is_empty() {
            let puffs = fx.puffs(&world, world.tick() as f32 - 0.5, [12.0, 30.0, 20.0]);
            assert!(puffs.iter().any(|p| p.stretch != [0.0; 3]), "the tracer is among the effects");
            return;
        }
    }
    panic!("the tank never fired");
}

#[test]
fn a_streak_draws_long_and_fades_towards_its_tail() {
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
    // A streak from (10, 12) back to (14, 12), half a cell up.
    let head = [10.0, 12.0, 0.5];
    renderer.set_effects(&[Puff::streak(head, 0.3, [255, 230, 150, 255], 1.0, [4.0, 0.0, 0.0])]);
    let image = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let lift = |x: f32| {
        let (sx, sy) = camera.project(world.map(), [x, 12.0, 0.5], w as f32, h as f32).unwrap();
        let i = ((sy as u32) * w + sx as u32) as usize * 4;
        (0..3).map(|c| image[i + c] as i32 - plain[i + c] as i32).sum::<i32>()
    };
    let (at_head, middle, near_tail, beyond) = (lift(10.2), lift(12.0), lift(13.6), lift(15.0));
    assert!(at_head > 200, "bright at the head: {at_head}");
    assert!(middle > 40 && near_tail > 10, "it runs the whole way: {middle} {near_tail}");
    assert!(at_head > middle && middle > near_tail, "fading: {at_head} {middle} {near_tail}");
    assert!(beyond.abs() < 5, "and stops at its end: {beyond}");
    // Seen end on, it is a round blob: the same streak pointing at the camera covers little more than its width.
    let towards = {
        let eye = camera.pose().eye;
        let d = [eye[0] - head[0], eye[1] - head[1], eye[2] - head[2]];
        let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        [d[0] / n, d[1] / n, d[2] / n]
    };
    renderer.set_effects(&[Puff::streak(head, 0.3, [255, 230, 150, 255], 1.0, towards)]);
    let end_on = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let lit = |img: &[u8]| {
        (0..(w * h) as usize)
            .filter(|&i| (0..3).map(|c| img[i * 4 + c] as i32 - plain[i * 4 + c] as i32).sum::<i32>() > 30)
            .count()
    };
    assert!(lit(&end_on) * 3 < lit(&image), "{} {}", lit(&end_on), lit(&image));
}
