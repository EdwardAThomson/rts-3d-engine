//! The art studio's models: read from their glTF files, placed on the units they stand for, and drawn.

use ai3d::skirmish::{self, BUILDER, EXTRACTOR, FACTORY, GENERATOR, KINDS, TANK};
use render3d::Renderer;
use render3d::model::{Model, Models, piece_matrix};
use render3d::shapes::{Part, Shapes};
use rts_platform::Gpu;
use rts_platform::gpu::OFFSCREEN_FORMAT;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, World};
use view3d::camera::Camera;
use view3d::maths::transform;

#[test]
fn every_skirmish_kind_has_a_light_model_with_team_paint() {
    let models = Models::skirmish();
    assert_eq!(models.by_kind.len(), KINDS.len());
    for (kind, m) in models.by_kind.iter().enumerate() {
        let m = m.as_ref().unwrap_or_else(|| panic!("{} has a model", KINDS[kind]));
        assert!((300..2000).contains(&m.triangles()), "{} has {} triangles", KINDS[kind], m.triangles());
        assert!(m.pieces.iter().any(|p| p.vertices.iter().all(|v| v.team == 1.0)), "{} has team paint", KINDS[kind]);
        assert!(m.pieces.iter().any(|p| p.vertices.iter().all(|v| v.team == 0.0)), "and paint of its own");
        assert!(m.images.iter().all(|i| i.rgba.len() == (i.width * i.height * 4) as usize && i.width >= 64));
        // Standing on the ground, up the right way.
        assert!(m.min[2].abs() < 0.1 && m.max[2] > 2.0, "{} from {:?} to {:?}", KINDS[kind], m.min, m.max);
    }
    let tank = models.by_kind[TANK].as_ref().unwrap();
    assert!(tank.pieces.iter().any(|p| p.turns), "the tank's turret turns on its own");
    // The model's front is north: the gun reaches further forward than the hull does back.
    assert!(-tank.min[1] > tank.max[1], "the gun points north ({:?}, {:?})", tank.min, tank.max);
    assert!(models.by_kind[FACTORY].as_ref().unwrap().pieces.iter().all(|p| !p.turns));
}

#[test]
fn a_bad_file_or_list_is_an_error() {
    assert!(Model::from_glb(b"not a model").is_err());
    let tank = include_bytes!("../../../assets/skirmish/models/tank.glb");
    assert!(Model::from_glb(&tank[..tank.len() / 2]).is_err(), "a file cut short");
    let read = |_: &str| Some(tank.to_vec());
    assert!(Models::from_list(r#"{"metres_per_cell": 10, "kinds": {"tank": "t.glb"}}"#, &KINDS, read).is_ok());
    let unknown = Models::from_list(r#"{"metres_per_cell": 10, "kinds": {"dragon": "t.glb"}}"#, &KINDS, read);
    assert!(unknown.unwrap_err().contains("dragon"));
    let missing = Models::from_list(r#"{"metres_per_cell": 10, "kinds": {"tank": "t.glb"}}"#, &KINDS, |_| None);
    assert!(missing.is_err());
    assert!(Models::from_list(r#"{"kinds": {}}"#, &KINDS, read).is_err(), "no size");
}

/// Two tanks of each of two players facing each other across a flat field, and a builder.
fn field() -> World {
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    world.set_store(0, vec![1_000_000, 1_000_000], vec![2_000_000, 2_000_000]);
    for (owner, x) in [(0, 8), (0, 9), (1, 15), (1, 16)] {
        world.spawn_for(owner, TANK, x * SUB, 12 * SUB);
    }
    world.spawn_for(0, BUILDER, 6 * SUB, 8 * SUB);
    world
}

#[test]
fn models_are_one_size_per_metre_and_buildings_fit_their_footprints() {
    let models = Models::skirmish();
    let mut world = field();
    let builder = world.units().iter().find(|u| u.kind == BUILDER).unwrap().id;
    let all = Shapes::default().shapes(&world, 1.0);
    let tank = all.iter().find(|s| s.unit.is_some_and(|u| u.kind == TANK)).unwrap();
    let at = models.placement(tank).unwrap();
    assert_eq!(at.across, 1.0 / models.metres_per_cell);
    assert_eq!(at.up, at.across);
    assert_eq!(at.origin, view3d::to_view(world.unit(tank_id(tank)).unwrap().pos), "on the unit's own point");
    let plain = render3d::Shape::plain(Part::Spot(0, 0), [0.0; 3], [1.0; 3], [0; 4]);
    assert_eq!(models.placement(&plain), None, "a shape that is no unit has no model");

    // A 2 by 2 generator keeps its size; the extractor's model is made for 2 by 2 and shrinks to fit 1 by 1.
    for (kind, cx) in [(GENERATOR, 4), (EXTRACTOR, 12)] {
        world.command(Command::Build { unit: builder, kind, cx, cy: 3 });
        for _ in 0..4000 {
            world.step();
        }
    }
    let shapes = Shapes::default().shapes(&world, 1.0);
    for kind in [GENERATOR, EXTRACTOR] {
        let s = shapes.iter().find(|s| s.unit.is_some_and(|u| u.kind == kind)).expect("built");
        let m = models.by_kind[kind].as_ref().unwrap();
        let at = models.placement(s).unwrap();
        for a in 0..2 {
            let drawn = (m.max[a] - m.min[a]) * at.across;
            assert!(drawn <= s.max[a] - s.min[a] + 1e-4, "{} is {drawn} across in {:?}", KINDS[kind], s);
        }
        let shrunk = at.across < 1.0 / models.metres_per_cell;
        assert_eq!(shrunk, kind == EXTRACTOR, "{}", KINDS[kind]);
    }
}

fn tank_id(s: &render3d::Shape) -> u32 {
    match s.part {
        Part::Unit(id) => id,
        _ => panic!("a unit"),
    }
}

#[test]
fn a_frame_rises_as_it_is_built() {
    let models = Models::skirmish();
    let mut world = field();
    let builder = world.units().iter().find(|u| u.kind == BUILDER).unwrap().id;
    world.command(Command::Build { unit: builder, kind: FACTORY, cx: 3, cy: 2 });
    let mut ups = Vec::new();
    for _ in 0..6000 {
        world.step();
        if let Some(f) = Shapes::default().shapes(&world, 1.0).iter().find(|s| matches!(s.part, Part::Frame(_))) {
            let at = models.placement(f).unwrap();
            ups.push(at.up / at.across);
        }
    }
    assert!(ups.len() > 10 && ups[0] < 0.2 && *ups.last().unwrap() > 0.9, "from {} to {:?}", ups[0], ups.last());
    assert!(ups.windows(2).all(|w| w[1] >= w[0]));
}

#[test]
fn units_face_the_way_they_drive_and_turrets_their_target() {
    let models = Models::skirmish();
    let mut world = World::new(Heightmap::flat(32, 32, 0), skirmish::types(), 1);
    let tank = world.spawn_for(0, TANK, 4 * SUB, 16 * SUB);
    let mut shapes = Shapes::default();
    let pose = |shapes: &Shapes, world: &World| {
        shapes.shapes(world, 1.0).into_iter().find(|s| s.part == Part::Unit(tank)).unwrap().unit.unwrap()
    };
    // Before it moves, it faces the middle of the map: due east from here.
    assert!((pose(&shapes, &world).yaw - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    world.command(Command::Move { unit: tank, x: 4 * SUB, y: 4 * SUB });
    for _ in 0..30 {
        shapes.remember(&world);
        world.step();
    }
    let north = pose(&shapes, &world);
    assert!(north.yaw.abs() < 0.05, "driving north, it faces north ({})", north.yaw);
    assert_eq!(north.aim, north.yaw, "with its turret forward");
    world.command(Command::Stop { unit: tank });
    for _ in 0..5 {
        shapes.remember(&world);
        world.step();
    }
    assert!(pose(&shapes, &world).yaw.abs() < 0.05, "and keeps facing that way once it stops");

    // An enemy to the east: the turret turns to it while the hull stays put.
    let p = world.unit(tank).unwrap().pos;
    let enemy = world.spawn_for(1, TANK, p.x + 3 * SUB, p.y);
    world.command(Command::Attack { unit: tank, target: enemy });
    // The turret swings a quarter turn at its own rate.
    for _ in 0..12 {
        shapes.remember(&world);
        world.step();
    }
    let fighting = pose(&shapes, &world);
    assert!((fighting.aim - std::f32::consts::FRAC_PI_2).abs() < 0.05, "aims east ({})", fighting.aim);

    // The turret's pieces turn with the aim; the hull's with the heading.
    let shape = shapes.shapes(&world, 1.0).into_iter().find(|s| s.part == Part::Unit(tank)).unwrap();
    let at = models.placement(&shape).unwrap();
    let m = models.by_kind[TANK].as_ref().unwrap();
    let turret = m.pieces.iter().find(|p| p.turns).unwrap();
    let hull = m.pieces.iter().find(|p| !p.turns).unwrap();
    // A point ahead of each piece's own middle: the hull's lies north of the unit, the turret's east of it.
    let ahead = |piece: &render3d::model::Piece| {
        let q = transform(&piece_matrix(&at, &fighting, piece), [0.0, -4.0, 0.0]);
        let o = transform(&piece_matrix(&at, &fighting, piece), [0.0, 0.0, 0.0]);
        (q[0] - o[0], q[1] - o[1])
    };
    let (hx, hy) = ahead(hull);
    assert!(hy < 0.0 && hx.abs() < 0.05 * -hy, "the hull faces north ({hx}, {hy})");
    let (tx, ty) = ahead(turret);
    assert!(tx > 0.0 && ty.abs() < 1e-2, "the turret faces east ({tx}, {ty})");
}

/// Which of the first two players' colours a pixel leans towards, if either.
fn leans([r, g, b]: [i32; 3]) -> Option<u8> {
    if b > r + 30 && b > g + 20 {
        Some(0)
    } else if r > b + 30 && r > g + 30 {
        Some(1)
    } else {
        None
    }
}

#[test]
fn models_draw_in_their_owners_colours_where_the_boxes_were() {
    let gpu = Gpu::headless().expect("a GPU adapter (a software one will do)");
    let world = field();
    let mut camera = Camera::new(world.map());
    camera.zoom = 0.15;
    camera.focus = [12.0, 12.0, 0.0];
    camera.settle(world.map());
    let shapes = Shapes::default().shapes(&world, 1.0);
    let (w, h) = (640, 360);
    let mut renderer = Renderer::new(&gpu, OFFSCREEN_FORMAT);
    let boxes = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    renderer.set_models(&gpu, Models::skirmish());
    let image = renderer.draw_to_image(&gpu, (w, h), &world, &camera, &shapes, [20, 24, 32]);
    let count =
        |img: &[u8], p: u8| img.chunks(4).filter(|c| leans([c[0], c[1], c[2]].map(i32::from)) == Some(p)).count();
    let (blue, red) = (count(&image, 0), count(&image, 1));
    assert!(blue > 100 && red > 100, "each side's paint shows: {blue} blue, {red} red");
    // Paint only covers part of a model, so there is less of each colour than with plain boxes.
    assert!(blue < count(&boxes, 0) && red < count(&boxes, 1));
    // The baked texture brings colours the boxes never had: many more distinct ones.
    let distinct =
        |img: &[u8]| img.chunks(4).map(|c| (c[0], c[1], c[2])).collect::<std::collections::BTreeSet<_>>().len();
    assert!(distinct(&image) > distinct(&boxes) + 200, "{} against {}", distinct(&image), distinct(&boxes));
}

#[test]
fn mipmaps_halve_down_to_one_pixel_and_average() {
    use render3d::model::Image;
    let rgba = (0..16u8).flat_map(|i| [i * 16, 0, 255 - i * 16, 255]).collect();
    let levels = render3d::model::mipmaps(&Image { width: 4, height: 4, rgba });
    assert_eq!(levels.iter().map(|l| (l.width, l.height)).collect::<Vec<_>>(), [(4, 4), (2, 2), (1, 1)]);
    // The first pixel of level 1 averages pixels 0, 1, 4 and 5.
    assert_eq!(levels[1].rgba[..4], [(16 + 64 + 80 + 2) / 4, 0, ((255 + 239 + 191 + 175 + 2) / 4) as u8, 255]);
}

#[test]
fn wrecks_lie_where_they_block_and_vehicles_ride_over_heaps() {
    use render3d::shapes::{HEAP, WRECK_SHARE};
    let models = Models::skirmish();
    let mut world = World::new(Heightmap::flat(24, 24, 0), skirmish::types(), 1);
    let generator = world.spawn_for(1, GENERATOR, 10 * SUB, 10 * SUB);
    let enemy_tank = world.spawn_for(1, TANK, 15 * SUB, 14 * SUB);
    let tanks = [9, 10, 11].map(|x| world.spawn_for(0, TANK, x * SUB, 14 * SUB));
    for t in tanks {
        world.command(Command::Attack { unit: t, target: generator });
    }
    let mut shapes = Shapes::default();
    for _ in 0..2000 {
        shapes.remember(&world);
        world.step();
        if world.unit(generator).is_none() && world.unit(enemy_tank).is_none() {
            break;
        }
    }
    assert!(world.wrecks().iter().any(|w| w.kind == GENERATOR) && world.wrecks().iter().any(|w| w.kind == TANK));
    let tank = *tanks.iter().find(|&&t| world.unit(t).is_some()).expect("a tank of ours is left");
    let all = shapes.shapes(&world, 1.0);
    let wrecks: Vec<_> = all.iter().filter(|s| matches!(s.part, Part::Wreck(_))).collect();
    // The generator's wreck covers the 2 by 2 cells it blocks, drawn as its model, burnt and low.
    let rubble = wrecks.iter().find(|s| s.unit.is_some_and(|u| u.structure)).unwrap();
    assert_eq!((rubble.min[0], rubble.min[1], rubble.max[0], rubble.max[1]), (9.0, 9.0, 11.0, 11.0));
    assert!((9..11).all(|c| world.is_blocked(c, 9) && world.is_blocked(c, 10)));
    let at = models.placement(rubble).unwrap();
    assert!((at.up / at.across - WRECK_SHARE).abs() < 1e-6, "squashed");

    // The tank's wreck is a low heap; the tank drives onto it and rides up over it.
    let heap = wrecks.iter().find(|s| s.unit.is_some_and(|u| !u.structure)).unwrap();
    assert!((heap.max[2] - heap.min[2] - HEAP).abs() < 1e-6);
    let w = world.wrecks().iter().find(|w| w.kind == TANK && w.pos.x > 12 * SUB).expect("the enemy tank's").pos;
    let ground = |s: &Shapes, world: &World| {
        s.shapes(world, 1.0).into_iter().find(|s| s.part == Part::Unit(tank)).unwrap().min[2]
    };
    assert_eq!(ground(&shapes, &world), 0.0, "on the ground away from it");
    world.command(Command::Move { unit: tank, x: w.x, y: w.y });
    for _ in 0..600 {
        shapes.remember(&world);
        world.step();
    }
    let p = world.unit(tank).unwrap().pos;
    assert_eq!((p.x, p.y), (w.x, w.y), "the heap blocks nothing");
    assert!((ground(&shapes, &world) - HEAP).abs() < 1e-6, "and the tank sits on top of it");
}
