use rts_core::hash::hash_of;
use rts_core::rng::{random_int, seed_state};
use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const TRACKED: usize = 0;
const AIR: usize = 1;

fn classes() -> Vec<UnitType> {
    vec![
        UnitType {
            movement: MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: 64 },
            max_health: 100,
            height: 64,
            weapon: None,
            armour: 0,
            production: Production::default(),
            structure: None,
        },
        UnitType {
            movement: MoveClass { speed: 32, max_slope: None, climb_slowdown: 0, altitude: 300, radius: 64 },
            max_health: 100,
            height: 64,
            weapon: None,
            armour: 0,
            production: Production::default(),
            structure: None,
        },
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Run until every move has ended or `limit` ticks pass. Returns each tick's events, in order.
fn run(world: &mut World, limit: u32) -> Vec<(u32, Event)> {
    let mut all = Vec::new();
    for _ in 0..limit {
        let tick = world.tick();
        all.extend(world.step().into_iter().map(|e| (tick, e)));
        if world.units().iter().all(|u| u.goal.is_none()) {
            break;
        }
    }
    all
}

/// A 9 by 9 map split by a north-south wall 2000 high along corner columns 4 and 5. With `gap`, the wall stops
/// short of the bottom two rows of cells.
fn walled(gap: bool) -> Heightmap {
    let mut corners = vec![0; 10 * 10];
    for cy in 0..10 {
        if gap && cy >= 7 {
            continue;
        }
        corners[cy * 10 + 4] = 2000;
        corners[cy * 10 + 5] = 2000;
    }
    Heightmap::new(9, 9, corners)
}

#[test]
fn a_unit_crosses_flat_ground_at_its_speed() {
    let mut world = World::new(Heightmap::flat(12, 3, 40), classes(), 1);
    let (x, y) = centre(0, 1);
    let id = world.spawn(TRACKED, x, y);
    assert_eq!(world.unit(id).unwrap().pos.z, 40, "units stand on the ground");
    world.command(Command::Move { unit: id, x: x + 10 * SUB, y });
    let events = run(&mut world, 500);
    // 2560 sub-cell units at 32 a tick is 80 ticks: the 80th step (tick 79) lands on the goal.
    assert_eq!(events, vec![(79, Event::MoveEnded { unit: id, reason: MoveEnd::Arrived, x: x + 10 * SUB, y })]);
    let unit = world.unit(id).unwrap();
    assert_eq!((unit.pos.x, unit.pos.y, unit.pos.z), (x + 10 * SUB, y, 40));
}

#[test]
fn ground_units_go_round_a_wall_and_aircraft_fly_over_it() {
    let mut world = World::new(walled(true), classes(), 1);
    let (sx, sy) = centre(1, 1);
    let (gx, gy) = centre(7, 1);
    let tank = world.spawn(TRACKED, sx, sy);
    let plane = world.spawn(AIR, sx, sy);
    world.command(Command::Move { unit: tank, x: gx, y: gy });
    world.command(Command::Move { unit: plane, x: gx, y: gy });

    let mut arrived = [None, None];
    let mut plane_top = 0;
    for _ in 0..600 {
        let tick = world.tick();
        for event in world.step() {
            let Event::MoveEnded { unit, reason, .. } = event else { continue };
            assert_eq!(reason, MoveEnd::Arrived);
            arrived[(unit - 1) as usize] = Some(tick);
        }
        let t = world.unit(tank).unwrap().pos;
        let (cx, cy) = (t.x / SUB, t.y / SUB);
        assert!(!((3..=5).contains(&cx) && cy <= 6), "the tank drove onto the wall at cell ({cx}, {cy})");
        assert_eq!(t.z, world.map().sample(t.x, t.y));
        let p = world.unit(plane).unwrap().pos;
        assert_eq!(p.z, world.map().sample(p.x, p.y) + 300, "aircraft keep their height above the ground");
        plane_top = plane_top.max(p.z);
        if arrived.iter().all(Option::is_some) {
            break;
        }
    }
    let [Some(tank_at), Some(plane_at)] = arrived else { panic!("both should arrive: {arrived:?}") };
    assert_eq!(plane_at, 47, "the plane flies straight: 1536 units at 32 a tick");
    assert!(tank_at > 2 * plane_at, "the tank's detour through the gap is long ({tank_at} ticks)");
    assert_eq!(plane_top, 2300, "the plane climbed over the wall");
}

#[test]
fn a_goal_behind_an_unbroken_wall_is_unreachable() {
    let mut world = World::new(walled(false), classes(), 1);
    let (sx, sy) = centre(1, 4);
    let tank = world.spawn(TRACKED, sx, sy);
    let (gx, gy) = centre(7, 4);
    world.command(Command::Move { unit: tank, x: gx, y: gy });
    let events = run(&mut world, 50);
    assert_eq!(events, vec![(0, Event::MoveEnded { unit: tank, reason: MoveEnd::Unreachable, x: sx, y: sy })]);
    assert_eq!(world.tick(), 1, "the move ends at once");
}

#[test]
fn climbing_is_slower_than_descending() {
    // A gentle ramp rising 16 height units per cell: a quarter of the tracked class's limit, so it keeps 88% of
    // its speed going up and all of it going down.
    let corners: Vec<i32> = (0..2).flat_map(|_| (0..13).map(|cx| 16 * cx)).collect();
    let ramp = Heightmap::new(12, 1, corners);
    let ticks = |from: i32, to: i32| {
        let mut world = World::new(ramp.clone(), classes(), 1);
        let (x, y) = centre(from, 0);
        let id = world.spawn(TRACKED, x, y);
        world.command(Command::Move { unit: id, x: centre(to, 0).0, y });
        let events = run(&mut world, 500);
        assert!(matches!(events[..], [(_, Event::MoveEnded { reason: MoveEnd::Arrived, .. })]));
        world.tick()
    };
    let (up, down) = (ticks(1, 11), ticks(11, 1));
    assert_eq!(down, 80);
    assert_eq!(up, 92, "28 a tick uphill: 2560 / 28 rounded up");
}

#[test]
fn stop_ends_a_move_where_the_unit_stands() {
    let mut world = World::new(Heightmap::flat(8, 8, 0), classes(), 1);
    let (x, y) = centre(0, 0);
    let id = world.spawn(TRACKED, x, y);
    world.command(Command::Move { unit: id, x: centre(7, 0).0, y });
    for _ in 0..5 {
        assert!(world.step().is_empty());
    }
    world.command(Command::Stop { unit: id });
    assert!(world.step().is_empty(), "stopping is not a move ending");
    let unit = world.unit(id).unwrap();
    assert_eq!((unit.pos.x, unit.goal), (x + 5 * 32, None));
}

/// A rolling 24 by 24 map from the seeded generator, with several units ordered about at different ticks.
fn skirmish() -> World {
    let mut rng = seed_state(7);
    let corners = (0..25 * 25).map(|_| random_int(&mut rng, 40) as i32).collect();
    let mut world = World::new(Heightmap::new(24, 24, corners), classes(), 1);
    for i in 0..6 {
        let (x, y) = centre(2 + 3 * i, 2 + i);
        world.spawn(if i % 3 == 2 { AIR } else { TRACKED }, x, y);
    }
    world
}

fn skirmish_orders(world: &mut World) {
    match world.tick() {
        0 => {
            for id in 1..=6 {
                world.command(Command::Move { unit: id, x: 20 * SUB, y: 20 * SUB });
            }
        }
        40 => world.command(Command::Move { unit: 2, x: 3 * SUB + 17, y: 21 * SUB + 200 }),
        90 => world.command(Command::Stop { unit: 4 }),
        _ => {}
    }
}

#[test]
fn a_replay_of_the_command_log_matches_tick_for_tick() {
    let mut live = skirmish();
    let mut hashes = Vec::new();
    let mut arrived = Vec::new();
    for _ in 0..400 {
        skirmish_orders(&mut live);
        for (unit, reason) in move_ends(live.step()) {
            assert_eq!(reason, MoveEnd::Arrived);
            arrived.push(unit);
        }
        hashes.push(hash_of(&live).value());
    }

    assert_eq!(arrived, vec![6, 5, 3, 2, 1], "aircraft first, then the tanks; unit 4 was stopped");

    let mut replay = skirmish();
    let log = live.command_log().to_vec();
    for (i, expected) in hashes.iter().enumerate() {
        let tick = replay.tick();
        for logged in log.iter().filter(|l| l.tick == tick) {
            replay.command(logged.command.clone());
        }
        replay.step();
        assert_eq!(hash_of(&replay).value(), *expected, "replay diverged at tick {i}");
    }

    // The golden hash pins today's movement rules. If a change moves it, say so and update it on purpose.
    assert_eq!(hash_of(&live).hex(), "1159a708");
}

/// The move endings among a tick's events.
fn move_ends(events: Vec<Event>) -> Vec<(u32, MoveEnd)> {
    events
        .into_iter()
        .filter_map(|e| match e {
            Event::MoveEnded { unit, reason, .. } => Some((unit, reason)),
            _ => None,
        })
        .collect()
}

/// A move and its mirror image, turned half a turn about the middle of the map, stay mirror images tick by tick:
/// rounding and route choices favour no direction, so no corner of a mirrored map has an edge.
#[test]
fn a_move_and_its_mirror_image_stay_mirror_images() {
    let size = 32;
    let mirror = |(x, y): (i32, i32)| (size * SUB - x, size * SUB - y);
    let mut rng = seed_state(5);
    let mut cell = || (random_int(&mut rng, size as u32) as i32, random_int(&mut rng, size as u32) as i32);
    let mut pairs = vec![((5, 7), (26, 20)), ((3, 3), (28, 9)), ((10, 2), (12, 29))];
    pairs.extend((0..20).map(|_| (cell(), cell())));
    for (from, to) in pairs {
        let mut a = World::new(Heightmap::flat(size, size, 0), classes(), 1);
        let mut b = World::new(Heightmap::flat(size, size, 0), classes(), 1);
        let (fa, ta) = (centre(from.0, from.1), centre(to.0, to.1));
        let (fb, tb) = (mirror(fa), mirror(ta));
        let ua = a.spawn(TRACKED, fa.0, fa.1);
        let ub = b.spawn(TRACKED, fb.0, fb.1);
        a.command(Command::Move { unit: ua, x: ta.0, y: ta.1 });
        b.command(Command::Move { unit: ub, x: tb.0, y: tb.1 });
        for _ in 0..400 {
            a.step();
            b.step();
            let (pa, pb) = (a.unit(ua).unwrap().pos, b.unit(ub).unwrap().pos);
            assert_eq!(mirror((pa.x, pa.y)), (pb.x, pb.y), "{from:?} to {to:?}, tick {}", a.tick());
        }
    }
}

/// The same for a group that packs round its goal: steering pushes favour no direction either.
#[test]
fn a_group_move_and_its_mirror_image_stay_mirror_images() {
    let size = 32;
    let mirror = |(x, y): (i32, i32)| (size * SUB - x, size * SUB - y);
    let mut a = World::new(Heightmap::flat(size, size, 0), classes(), 1);
    let mut b = World::new(Heightmap::flat(size, size, 0), classes(), 1);
    let goal = centre(24, 22);
    let mut ids = Vec::new();
    for i in 0..9 {
        let p = centre(4 + i % 3, 5 + i / 3);
        let q = mirror(p);
        ids.push((a.spawn(TRACKED, p.0, p.1), b.spawn(TRACKED, q.0, q.1)));
    }
    let g = mirror(goal);
    for &(ia, ib) in &ids {
        a.command(Command::Move { unit: ia, x: goal.0, y: goal.1 });
        b.command(Command::Move { unit: ib, x: g.0, y: g.1 });
    }
    for _ in 0..600 {
        a.step();
        b.step();
        for &(ia, ib) in &ids {
            let (pa, pb) = (a.unit(ia).unwrap().pos, b.unit(ib).unwrap().pos);
            assert_eq!(mirror((pa.x, pa.y)), (pb.x, pb.y), "unit {ia}, tick {}", a.tick());
        }
    }
}
