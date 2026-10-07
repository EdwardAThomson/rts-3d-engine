use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::space::{SUB, Vec3};
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const TRACKED: usize = 0;
const AIR: usize = 1;
const RADIUS: i32 = 64;

fn classes() -> Vec<UnitType> {
    vec![
        UnitType {
            movement: MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: RADIUS },
            max_health: 100,
            height: 64,
            weapon: None,
            armour: 0,
            production: Production::default(),
            structure: None,
        },
        UnitType {
            movement: MoveClass { speed: 32, max_slope: None, climb_slowdown: 0, altitude: 300, radius: RADIUS },
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

fn gap(a: Vec3, b: Vec3) -> i32 {
    a.ground_distance(b) as i32
}

/// The smallest distance between any two ground units.
fn closest_pair(world: &World) -> i32 {
    let units = world.units();
    let mut best = i32::MAX;
    for (i, a) in units.iter().enumerate() {
        for b in &units[i + 1..] {
            if a.kind == b.kind {
                best = best.min(gap(a.pos, b.pos));
            }
        }
    }
    best
}

/// Step until no unit has a move left or `limit` ticks pass; returns who arrived and who did not.
fn run(world: &mut World, limit: u32) -> (Vec<u32>, Vec<u32>) {
    let (mut arrived, mut failed) = (Vec::new(), Vec::new());
    for _ in 0..limit {
        for (unit, reason) in move_ends(world.step()) {
            match reason {
                MoveEnd::Arrived => arrived.push(unit),
                MoveEnd::Unreachable => failed.push(unit),
            }
        }
        if world.units().iter().all(|u| u.goal.is_none()) {
            break;
        }
    }
    (arrived, failed)
}

#[test]
fn units_spawned_on_one_point_part_at_once() {
    let mut world = World::new(Heightmap::flat(6, 6, 0), classes(), 1);
    let (x, y) = centre(3, 3);
    for _ in 0..2 {
        world.spawn(TRACKED, x, y);
    }
    world.step();
    assert!(closest_pair(&world) >= 2 * RADIUS, "still overlapping: {}", closest_pair(&world));
}

#[test]
fn a_group_sent_to_one_point_packs_round_it_and_every_unit_arrives() {
    let mut world = World::new(Heightmap::flat(24, 24, 0), classes(), 1);
    let mut ids = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let (x, y) = centre(2 + i, 2 + j);
            ids.push(world.spawn(TRACKED, x, y));
        }
    }
    let goal = centre(18, 18);
    for &id in &ids {
        world.command(Command::Move { unit: id, x: goal.0, y: goal.1 });
    }
    let (mut arrived, failed) = run(&mut world, 1000);
    arrived.sort_unstable();
    assert_eq!((arrived, failed), (ids.clone(), vec![]), "every unit's move ends by arriving");
    assert!(world.tick() < 1000);
    // The last arrivals may still overlap a little; resting units then settle apart and stay put.
    for _ in 0..20 {
        assert!(world.step().is_empty());
    }
    let settled: Vec<Vec3> = world.units().iter().map(|u| u.pos).collect();
    world.step();
    assert_eq!(world.units().iter().map(|u| u.pos).collect::<Vec<_>>(), settled, "the pack keeps jostling");
    // Nine discs of radius 64 pack within about three diameters of the goal, and none overlaps another by more
    // than rounding.
    let g = Vec3::new(goal.0, goal.1, 0);
    for u in world.units() {
        assert!(gap(u.pos, g) <= 6 * RADIUS, "unit {} stopped {} from the goal", u.id, gap(u.pos, g));
    }
    assert!(closest_pair(&world) >= 2 * RADIUS - 4, "units overlap: {}", closest_pair(&world));
}

#[test]
fn units_meeting_head_on_slide_past_each_other() {
    let mut world = World::new(Heightmap::flat(16, 5, 0), classes(), 1);
    let (west, east) = (centre(1, 2), centre(14, 2));
    let a = world.spawn(TRACKED, west.0, west.1);
    let b = world.spawn(TRACKED, east.0, east.1);
    world.command(Command::Move { unit: a, x: east.0, y: east.1 });
    world.command(Command::Move { unit: b, x: west.0, y: west.1 });
    let (mut arrived, _) = run(&mut world, 400);
    arrived.sort_unstable();
    assert_eq!(arrived, vec![a, b]);
    assert!(world.tick() < 120, "the swap took {} ticks; straight driving is 104", world.tick());
    let pos = |id| world.unit(id).unwrap().pos;
    assert_eq!((pos(a).x, pos(a).y), east);
    assert_eq!((pos(b).x, pos(b).y), west);
}

#[test]
fn a_moving_unit_shoves_an_idle_one_out_of_its_way() {
    let mut world = World::new(Heightmap::flat(12, 5, 0), classes(), 1);
    let (start, goal) = (centre(1, 2), centre(10, 2));
    let mover = world.spawn(TRACKED, start.0, start.1);
    let idle_at = centre(5, 2);
    let idle = world.spawn(TRACKED, idle_at.0, idle_at.1);
    world.command(Command::Move { unit: mover, x: goal.0, y: goal.1 });
    let (arrived, _) = run(&mut world, 400);
    assert_eq!(arrived, vec![mover]);
    let m = world.unit(mover).unwrap().pos;
    assert_eq!((m.x, m.y), goal, "the mover was not slowed off its line");
    let i = world.unit(idle).unwrap();
    assert!(i.goal.is_none());
    assert_ne!((i.pos.x, i.pos.y), idle_at, "the idle unit was pushed aside");
}

#[test]
fn pushes_never_put_a_unit_on_ground_it_cannot_cross() {
    // A cliff 2000 high along corner columns 4 and 5, with a one-cell gap in the middle row. Six tanks squeeze
    // through it together.
    let mut corners = vec![0; 10 * 10];
    for cy in 0..10 {
        if cy == 4 || cy == 5 {
            continue;
        }
        corners[cy * 10 + 4] = 2000;
        corners[cy * 10 + 5] = 2000;
    }
    let mut world = World::new(Heightmap::new(9, 9, corners), classes(), 1);
    let cliff = |p: Vec3| {
        let (cx, cy) = (p.x / SUB, p.y / SUB);
        (3..=5).contains(&cx) && cy != 4
    };
    let mut ids = Vec::new();
    for j in 0..6 {
        let (x, y) = centre(1, 1 + j);
        ids.push(world.spawn(TRACKED, x, y));
    }
    let goal = centre(7, 4);
    for &id in &ids {
        world.command(Command::Move { unit: id, x: goal.0, y: goal.1 });
    }
    let mut arrived = Vec::new();
    for _ in 0..1000 {
        for (unit, reason) in move_ends(world.step()) {
            assert_eq!(reason, MoveEnd::Arrived);
            arrived.push(unit);
        }
        for u in world.units() {
            assert!(!cliff(u.pos), "unit {} was pushed onto the cliff at {:?}", u.id, u.pos);
        }
        if arrived.len() == ids.len() {
            break;
        }
    }
    arrived.sort_unstable();
    assert_eq!(arrived, ids);
}

#[test]
fn aircraft_and_ground_units_do_not_push_each_other() {
    let mut world = World::new(Heightmap::flat(6, 6, 0), classes(), 1);
    let (x, y) = centre(3, 3);
    let tank = world.spawn(TRACKED, x, y);
    let plane = world.spawn(AIR, x, y);
    world.step();
    for id in [tank, plane] {
        let p = world.unit(id).unwrap().pos;
        assert_eq!((p.x, p.y), (x, y));
    }
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
