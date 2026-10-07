use rts_core::hash::hash_of;
use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const BUILDER: usize = 0;
const PLANT: usize = 1;
const WALL: usize = 2;
const PLANE: usize = 3;
const TANK: usize = 4;

/// A builder adds 10 work a tick. A plant is a 2 by 2 structure costing 500 of the one resource over 500 work,
/// so one builder takes 50 ticks and pays 10 a tick; once finished it produces 3 a tick. A wall is 1 cell wide
/// and 10 deep and cannot be built, only placed. A tank's cannon takes 25 a shot.
fn types() -> Vec<UnitType> {
    let ground = MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 0, altitude: 0, radius: 64 };
    let still = MoveClass { speed: 0, ..ground.clone() };
    let plain = |movement: MoveClass, max_health, production, structure| UnitType {
        movement,
        max_health,
        height: 64,
        weapon: None,
        armour: 0,
        production,
        structure,
    };
    let cannon =
        Weapon { range: 1536, reload: 10, speed: 128, gravity: 0, damage: 25, splash: 0, scatter: 0, against: vec![] };
    vec![
        plain(ground.clone(), 100, Production { build_power: 10, builds: vec![PLANT], ..Default::default() }, None),
        plain(
            still.clone(),
            400,
            Production { cost: vec![500], build_time: 500, produces: vec![3], ..Default::default() },
            Some(Structure { width: 2, depth: 2, max_rise: 16 }),
        ),
        plain(still, 50, Production::default(), Some(Structure { width: 1, depth: 10, max_rise: 1000 })),
        plain(MoveClass { max_slope: None, altitude: 300, ..ground.clone() }, 100, Production::default(), None),
        UnitType { weapon: Some(cannon), ..plain(ground, 100, Production::default(), None) },
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn spawn(world: &mut World, kind: usize, cx: i32, cy: i32) -> u32 {
    let (x, y) = centre(cx, cy);
    world.spawn(kind, x, y)
}

/// Whether a point lies on the plant's site, cells (8, 8) to (9, 9).
fn on_site(x: i32, y: i32) -> bool {
    (8 * SUB..10 * SUB).contains(&x) && (8 * SUB..10 * SUB).contains(&y)
}

#[test]
fn a_builder_places_a_frame_beside_the_site_and_builds_it_paying_as_it_goes() {
    let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 1);
    world.set_store(0, vec![2000], vec![10_000]);
    let builder = spawn(&mut world, BUILDER, 1, 1);
    world.command(Command::Build { unit: builder, kind: PLANT, cx: 8, cy: 8 });
    let (mut placed, mut built) = (None, None);
    let mut health = Vec::new();
    for _ in 0..400 {
        let tick = world.tick();
        for e in world.step() {
            match e {
                Event::Placed { by, unit } => placed = Some((tick, by, unit)),
                Event::Built { by, unit } => built = Some((tick, by, unit)),
                _ => {}
            }
        }
        let b = world.unit(builder).unwrap().pos;
        assert!(!on_site(b.x, b.y), "the builder never stands on the site");
        if let Some((_, _, frame)) = placed
            && built.is_none()
        {
            health.push(world.unit(frame).unwrap().health);
        }
        if built.is_some() {
            break;
        }
    }
    let (placed_at, by, frame) = placed.expect("the frame was placed");
    assert_eq!(by, builder);
    let (built_at, by, unit) = built.expect("and finished");
    assert_eq!((by, unit), (builder, frame));
    assert_eq!(built_at - placed_at, 49, "500 work at 10 a tick, from the tick it was placed");
    assert!(health.windows(2).all(|w| w[0] < w[1]), "a frame's health grows as it is built");
    let plant = world.unit(frame).unwrap();
    assert_eq!((plant.health, plant.build, plant.pos.x, plant.pos.y), (400, None, 9 * SUB, 9 * SUB));
    assert_eq!(world.store(0).unwrap().amount, vec![1500], "it cost exactly its cost");
    world.step();
    assert_eq!(world.store(0).unwrap().amount, vec![1503], "and now it produces");
    assert_eq!(world.unit(builder).unwrap().assist, None, "the builder is free again");
}

#[test]
fn a_second_builder_assisting_halves_the_build_time() {
    let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 1);
    world.set_store(0, vec![2000], vec![10_000]);
    let a = spawn(&mut world, BUILDER, 6, 8);
    let b = spawn(&mut world, BUILDER, 11, 9);
    world.command(Command::Build { unit: a, kind: PLANT, cx: 8, cy: 8 });
    let mut frame = None;
    let mut times = Vec::new();
    for _ in 0..200 {
        let tick = world.tick();
        for e in world.step() {
            match e {
                Event::Placed { unit, .. } => {
                    frame = Some(unit);
                    world.command(Command::Assist { unit: b, target: unit });
                    times.push(tick);
                }
                Event::Built { .. } => times.push(tick),
                _ => {}
            }
        }
    }
    assert!(frame.is_some());
    let [placed, built] = times[..] else { panic!("{times:?}") };
    assert!(built - placed <= 27, "two builders take about 25 ticks, not 50 ({})", built - placed);
    assert_eq!(world.store(0).unwrap().amount[0], 2000 - 500 + 3 * (200 - built as i64 - 1), "paid once");
}

#[test]
fn builds_on_bad_sites_are_ignored() {
    // A cliff of 300 along corner column 12.
    let corners = (0..17).flat_map(|_| (0..17).map(|x| if x >= 12 { 300 } else { 0 })).collect();
    let mut world = World::new(Heightmap::new(16, 16, corners), types(), 1);
    let builder = spawn(&mut world, BUILDER, 1, 1);
    spawn(&mut world, WALL, 4, 5);
    for (cx, cy) in [(11, 3), (3, 3), (15, 15), (-1, 2)] {
        world.command(Command::Build { unit: builder, kind: PLANT, cx, cy });
        world.step();
        assert_eq!(world.unit(builder).unwrap().plan, None, "site ({cx}, {cy}) should be refused");
    }
    world.command(Command::Build { unit: builder, kind: WALL, cx: 6, cy: 1 });
    world.step();
    assert_eq!(world.unit(builder).unwrap().plan, None, "it cannot build walls");
    world.command(Command::Build { unit: builder, kind: PLANT, cx: 6, cy: 1 });
    world.step();
    assert_eq!(world.unit(builder).unwrap().plan, Some((PLANT, 6, 1)));
}

#[test]
fn a_builder_waits_for_the_site_to_clear() {
    let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 1);
    world.set_store(0, vec![2000], vec![10_000]);
    let builder = spawn(&mut world, BUILDER, 4, 8);
    let squatter = spawn(&mut world, BUILDER, 9, 9);
    world.command(Command::Build { unit: builder, kind: PLANT, cx: 8, cy: 8 });
    for _ in 0..100 {
        assert!(!world.step().iter().any(|e| matches!(e, Event::Placed { .. })), "never on top of a unit");
    }
    world.command(Command::Move { unit: squatter, x: centre(14, 14).0, y: centre(14, 14).1 });
    let placed = (0..100).any(|_| world.step().iter().any(|e| matches!(e, Event::Placed { .. })));
    assert!(placed, "the frame goes down once the site is clear");
}

#[test]
fn structures_block_ground_units_but_not_aircraft_and_stop_blocking_when_destroyed() {
    // A wall along column 8, rows 0 to 9, on a 16 by 12 map.
    let mut world = World::new(Heightmap::flat(16, 12, 0), types(), 1);
    let (wx, wy) = (8 * SUB + SUB / 2, 5 * SUB);
    let wall = world.spawn_for(1, WALL, wx, wy);
    assert_eq!((world.unit(wall).unwrap().pos.x, world.unit(wall).unwrap().pos.y), (wx, wy));
    let builder = spawn(&mut world, BUILDER, 2, 2);
    let plane = spawn(&mut world, PLANE, 2, 2);
    let goal = centre(13, 2);
    for u in [builder, plane] {
        world.command(Command::Move { unit: u, x: goal.0, y: goal.1 });
    }
    let mut arrived = Vec::new();
    for _ in 0..600 {
        let tick = world.tick();
        for e in world.step() {
            if let Event::MoveEnded { unit, reason: MoveEnd::Arrived, .. } = e {
                arrived.push((unit, tick));
            }
        }
        let p = world.unit(builder).unwrap().pos;
        assert!(!(p.x / SUB == 8 && p.y / SUB < 10), "the builder drove through the wall at {p:?}");
    }
    let tick_of = |id| arrived.iter().find(|(u, _)| *u == id).map(|&(_, t)| t).unwrap();
    assert_eq!(tick_of(plane), 87, "the plane flies straight over: 11 cells at 32 a tick");
    assert!(tick_of(builder) > 150, "the builder goes round the end of the wall");

    // Knock the wall down; the way straight across opens up.
    let tank = spawn(&mut world, TANK, 6, 5);
    world.command(Command::Attack { unit: tank, target: wall });
    let destroyed =
        (0..100).any(|_| world.step().iter().any(|e| matches!(e, Event::Destroyed { unit, .. } if *unit == wall)));
    assert!(destroyed);
    let back = centre(2, 2);
    world.command(Command::Move { unit: builder, x: back.0, y: back.1 });
    let start = world.tick();
    let mut back_at = None;
    for _ in 0..200 {
        let tick = world.tick();
        if world.step().iter().any(|e| matches!(e, Event::MoveEnded { unit, .. } if *unit == builder)) {
            back_at = Some(tick - start);
            break;
        }
    }
    let back_at = back_at.expect("the builder drives back");
    assert!(back_at < 100, "straight back through the gap: {back_at} ticks");
}

#[test]
fn a_replay_of_a_game_with_building_matches() {
    let start = || {
        let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 3);
        world.set_store(0, vec![900], vec![10_000]);
        spawn(&mut world, BUILDER, 2, 2);
        spawn(&mut world, BUILDER, 3, 5);
        world
    };
    let mut live = start();
    let mut hashes = Vec::new();
    for tick in 0..300 {
        match tick {
            0 => live.command(Command::Build { unit: 1, kind: PLANT, cx: 8, cy: 8 }),
            5 => live.command(Command::Build { unit: 2, kind: PLANT, cx: 4, cy: 10 }),
            _ => {}
        }
        live.step();
        hashes.push(hash_of(&live).value());
    }
    assert!(live.units().iter().filter(|u| u.kind == PLANT && u.build.is_none()).count() >= 1);
    let replay = Replay::record(start(), live.command_log().to_vec(), 300, 50);
    for tick in [1, 50, 120, 299, 300] {
        assert_eq!(hash_of(&replay.seek(tick)).value(), hashes[tick as usize - 1], "tick {tick}");
    }
}
