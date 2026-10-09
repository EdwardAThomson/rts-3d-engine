//! Fog of war on screen: the brightness of each cell for the side being played, enemies out of sight left out,
//! remembered structures drawn where they were last seen, effects and the ground darkened on the GPU.

use ai3d::skirmish::{self, BUILDER, GENERATOR, TANK};
use render3d::Renderer;
use render3d::effects::Puff;
use render3d::fog::{self, FOG, SHROUD, VISIBLE};
use render3d::shapes::{Part, Shapes};
use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::vision::FogRules;
use sim3d::world::{Command, World};
use view3d::camera::Camera;

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn field() -> World {
    let mut world = World::new(Heightmap::flat(32, 32, 0), skirmish::types(), 1);
    world.set_fog(Some(FogRules::default()));
    world
}

#[test]
fn each_cell_is_bright_in_sight_dim_in_fog_and_dark_in_shroud() {
    let mut world = field();
    let (x, y) = centre(6, 6);
    let tank = world.spawn(TANK, x, y);
    let (w, h, cells) = fog::brightness(&world, Some(0)).unwrap();
    assert_eq!((w, h, cells.len()), (32, 32, 32 * 32));
    assert_eq!(cells[6 * 32 + 6], VISIBLE);
    assert_eq!(cells[6 * 32 + 30], SHROUD);
    world.command(Command::Move { unit: tank, x: centre(26, 6).0, y });
    for _ in 0..800 {
        world.step();
    }
    let (_, _, cells) = fog::brightness(&world, Some(0)).unwrap();
    assert_eq!(cells[6 * 32 + 6], FOG, "where it was");
    assert_eq!(cells[6 * 32 + 26], VISIBLE, "where it is");
    // Watching, or a game without fog, draws everything bright.
    assert!(fog::brightness(&world, None).is_none());
    world.set_fog(None);
    assert!(fog::brightness(&world, Some(0)).is_none());
}

#[test]
fn the_view_leaves_out_unseen_enemies_and_keeps_remembered_buildings() {
    let mut world = field();
    let builder = world.spawn(BUILDER, centre(6, 6).0, centre(6, 6).1);
    let enemy = world.spawn_for(1, TANK, centre(25, 25).0, centre(25, 25).1);
    let plant = world.spawn_for(1, GENERATOR, centre(10, 6).0, centre(10, 6).1);
    world.step();
    let mut shapes = Shapes::default();
    shapes.viewer = Some(0);
    let parts =
        |shapes: &Shapes, world: &World| shapes.shapes(world, 1.0).into_iter().map(|s| s.part).collect::<Vec<_>>();
    let seen = parts(&shapes, &world);
    assert!(seen.contains(&Part::Unit(builder)) && seen.contains(&Part::Unit(plant)));
    assert!(!seen.contains(&Part::Unit(enemy)), "out of sight");
    // The builder goes; the plant is still drawn, as a ghost, where it was seen.
    world.command(Command::Move { unit: builder, x: centre(2, 28).0, y: centre(2, 28).1 });
    for _ in 0..800 {
        world.step();
    }
    let seen = parts(&shapes, &world);
    assert!(!seen.contains(&Part::Unit(plant)) && seen.contains(&Part::Ghost(plant)));
    // With no viewer, everything is drawn as it is.
    shapes.viewer = None;
    let all = parts(&shapes, &world);
    assert!(all.contains(&Part::Unit(enemy)) && all.contains(&Part::Unit(plant)));
    assert!(!all.iter().any(|p| matches!(p, Part::Ghost(_))));
}

#[test]
fn effects_out_of_sight_are_not_shown() {
    let mut world = field();
    world.spawn(TANK, centre(6, 6).0, centre(6, 6).1);
    let puff = |x: f32, y: f32| Puff::round([x, y, 0.2], 0.3, [255, 200, 100, 255], 1.0);
    let mut puffs = vec![puff(6.5, 6.5), puff(28.5, 28.5)];
    fog::visible_puffs(&world, Some(0), &mut puffs);
    assert_eq!(puffs.len(), 1);
    assert_eq!(puffs[0].at[0], 6.5);
}

#[test]
fn the_ground_is_dark_in_shroud_dim_in_fog_and_bright_in_sight() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let mut world = field();
    let tank = world.spawn(TANK, centre(8, 16).0, centre(8, 16).1);
    world.command(Command::Move { unit: tank, x: centre(16, 16).0, y: centre(16, 16).1 });
    for _ in 0..300 {
        world.step();
    }
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.8;
    camera.focus = [16.0, 16.0, 0.0];
    camera.settle(world.map());
    let (w, h) = (480u32, 270u32);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let fogged = fog::brightness(&world, Some(0)).unwrap();
    renderer.set_fog(Some((fogged.0, fogged.1, &fogged.2)));
    let image = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &[], [20, 24, 32]);
    renderer.set_fog(None);
    let open = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &[], [20, 24, 32]);
    let light = |img: &[u8], p: [f32; 2]| {
        let (x, y) = camera.project(world.map(), [p[0], p[1], 0.0], w as f32, h as f32).unwrap();
        assert!((0.0..w as f32).contains(&x) && (0.0..h as f32).contains(&y), "{p:?} on screen");
        let i = ((y as u32) * w + x as u32) as usize * 4;
        (0..3).map(|c| i32::from(img[i + c])).sum::<i32>()
    };
    let (sight, fog_, shroud) = ([16.5, 16.5], [7.5, 16.5], [27.5, 16.5]);
    assert_eq!(light(&image, sight), light(&open, sight), "in sight, as without fog");
    let (dim, full) = (light(&image, fog_), light(&open, fog_));
    assert!(dim < full * 3 / 5 && dim > full / 4, "fog dims the ground: {dim} of {full}");
    assert!(light(&image, shroud) < light(&open, shroud) / 10, "shroud is all but black");
}
