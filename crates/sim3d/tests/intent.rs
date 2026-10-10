use rts_core::hash::hash_of;
use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const FACTORY: usize = 0;
const SCOUT: usize = 1;
const GUNNER: usize = 2;

/// A factory (a mobile one, for simplicity) builds scouts that cost nothing and take 50 work, at 10 a tick. A
/// gunner has 100 health and a gun that does 10 a shot at up to 4 cells, every 10 ticks.
fn types() -> Vec<UnitType> {
    let class = |radius| MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 0, altitude: 0, radius, turn: 0 };
    let unit = |radius, production| UnitType {
        movement: class(radius),
        max_health: 100,
        height: 48,
        vision: 0,
        weapon: None,
        armour: 0,
        production,
        structure: None,
    };
    let gun = Weapon {
        range: 4 * SUB,
        reload: 10,
        speed: 128,
        gravity: 0,
        damage: 10,
        splash: 0,
        scatter: 0,
        against: vec![],
        turret: Some(0),
    };
    vec![
        unit(128, Production { build_power: 10, builds: vec![SCOUT, GUNNER], ..Default::default() }),
        unit(32, Production { build_time: 50, ..Default::default() }),
        UnitType { weapon: Some(gun), ..unit(64, Production { build_time: 100, ..Default::default() }) },
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn world() -> World {
    World::new(Heightmap::flat(24, 24, 0), types(), 1)
}

fn spawn(world: &mut World, owner: u8, kind: usize, cx: i32, cy: i32) -> u32 {
    let (x, y) = centre(cx, cy);
    world.spawn_for(owner, kind, x, y)
}

fn arrivals(events: &[(u32, Event)], unit: u32) -> Vec<(i32, i32)> {
    events
        .iter()
        .filter_map(|(_, e)| match *e {
            Event::MoveEnded { unit: u, reason: MoveEnd::Arrived, x, y } if u == unit => Some((x, y)),
            _ => None,
        })
        .collect()
}

fn run(world: &mut World, ticks: u32) -> Vec<(u32, Event)> {
    let mut all = Vec::new();
    for _ in 0..ticks {
        let tick = world.tick();
        all.extend(world.step().into_iter().map(|e| (tick, e)));
    }
    all
}

#[test]
fn a_patrol_goes_back_and_forth_until_given_another_order() {
    let mut world = world();
    let scout = spawn(&mut world, 0, SCOUT, 2, 2);
    let (bx, by) = centre(12, 2);
    world.command(Command::Patrol { unit: scout, x: bx, y: by });
    let events = run(&mut world, 400);
    let ends = arrivals(&events, scout);
    assert!(ends.len() >= 4, "{ends:?}");
    for (n, &(x, _)) in ends.iter().enumerate() {
        let near = if n % 2 == 0 { bx } else { centre(2, 2).0 };
        assert!((x - near).abs() <= SUB / 2, "leg {n} ended at {x}");
    }
    world.command(Command::Move { unit: scout, x: centre(6, 8).0, y: centre(6, 8).1 });
    let events = run(&mut world, 300);
    assert_eq!(arrivals(&events, scout).len(), 1, "a new order ends the patrol");
    assert_eq!(world.unit(scout).unwrap().patrol, None);
}

#[test]
fn a_patrol_stops_to_fight_and_then_carries_on() {
    let mut world = world();
    let gunner = spawn(&mut world, 0, GUNNER, 2, 10);
    let enemy = spawn(&mut world, 1, SCOUT, 12, 13);
    let (bx, by) = centre(20, 10);
    world.command(Command::Patrol { unit: gunner, x: bx, y: by });
    let events = run(&mut world, 600);
    let killed = events.iter().find(|(_, e)| matches!(e, Event::Destroyed { unit, .. } if *unit == enemy));
    assert!(killed.is_some(), "the patrol shot the scout on the way");
    let ends = arrivals(&events, gunner);
    assert!(ends.len() >= 2 && (ends[0].0 - bx).abs() <= SUB / 2, "and went on patrolling: {ends:?}");
}

#[test]
fn a_factory_keeps_the_count_it_was_told_and_replaces_losses() {
    let mut world = world();
    let a = spawn(&mut world, 0, FACTORY, 4, 4);
    let b = spawn(&mut world, 0, FACTORY, 4, 12);
    world.command(Command::Keep { unit: a, kind: SCOUT, count: 3 });
    world.command(Command::Keep { unit: b, kind: SCOUT, count: 3 });
    let events = run(&mut world, 300);
    let built = events.iter().filter(|(_, e)| matches!(e, Event::Built { .. })).count();
    let scouts = || world.units().iter().filter(|u| u.kind == SCOUT).count();
    assert_eq!((built, scouts()), (3, 3), "two factories share one target and never overshoot");

    // An enemy gunner shoots scouts; the factories build new ones, never holding more than three.
    spawn(&mut world, 1, GUNNER, 6, 8);
    let mut rebuilt = 0;
    for _ in 0..600 {
        for e in world.step() {
            if matches!(e, Event::Built { .. }) {
                rebuilt += 1;
            }
        }
        assert!(world.units().iter().filter(|u| u.kind == SCOUT).count() <= 3);
    }
    assert!(rebuilt >= 2, "losses were replaced ({rebuilt})");

    // A count of 0 drops the rule.
    world.command(Command::Keep { unit: a, kind: SCOUT, count: 0 });
    world.command(Command::Keep { unit: b, kind: SCOUT, count: 0 });
    run(&mut world, 60);
    let built = run(&mut world, 600).iter().filter(|(_, e)| matches!(e, Event::Built { .. })).count();
    assert_eq!(built, 0);
}

#[test]
fn a_hurt_unit_falls_back_once_when_its_health_drops_below_the_line() {
    let mut world = world();
    let gunner = spawn(&mut world, 0, GUNNER, 10, 10);
    let enemy = spawn(&mut world, 1, GUNNER, 13, 10);
    let home = centre(2, 2);
    world.command(Command::FallBack { unit: gunner, percent: 50, x: home.0, y: home.1 });
    let mut turned = None;
    for _ in 0..400 {
        world.step();
        let u = world.unit(gunner).unwrap();
        if turned.is_none() && u.goal.is_some() {
            turned = Some(u.health);
            assert_eq!(u.target, None, "it stopped fighting");
        }
    }
    assert_eq!(turned, Some(40), "it left on the hit that took it from 50 to 40");
    let u = world.unit(gunner).unwrap();
    assert!((u.pos.x - home.0).abs() <= SUB && (u.pos.y - home.1).abs() <= SUB, "and reached the point");
    assert!(world.unit(enemy).is_some());

    // Sent back in below the line, it fights on: the rule acts when health crosses the line, not on every hit.
    world.command(Command::Attack { unit: gunner, target: enemy });
    for _ in 0..400 {
        world.step();
        let (Some(u), Some(_)) = (world.unit(gunner), world.unit(enemy)) else { break };
        assert_eq!(u.target, Some(enemy), "health {}", u.health);
    }
}

#[test]
fn units_from_a_rallied_factory_head_for_the_rally_point() {
    let mut world = world();
    let f = spawn(&mut world, 0, FACTORY, 4, 4);
    let point = centre(16, 14);
    world.command(Command::Rally { unit: f, point: Some(point) });
    world.command(Command::Produce { unit: f, kind: SCOUT, repeat: false });
    world.command(Command::Produce { unit: f, kind: SCOUT, repeat: false });
    let events = run(&mut world, 500);
    let built: Vec<u32> = events
        .iter()
        .filter_map(|(_, e)| match *e {
            Event::Built { by, unit } if by == f => Some(unit),
            _ => None,
        })
        .collect();
    assert_eq!(built.len(), 2);
    for &id in &built {
        let u = world.unit(id).unwrap();
        assert!((u.pos.x - point.0).abs() <= SUB && (u.pos.y - point.1).abs() <= SUB, "scout {id} reached it");
    }

    // Dropping the rule leaves new units by the factory.
    world.command(Command::Rally { unit: f, point: None });
    world.command(Command::Produce { unit: f, kind: SCOUT, repeat: false });
    let events = run(&mut world, 200);
    let late = events.iter().find_map(|(_, e)| match *e {
        Event::Built { by, unit } if by == f => Some(unit),
        _ => None,
    });
    let u = world.unit(late.expect("a third scout")).unwrap();
    assert_eq!(u.goal, None);
    assert!((u.pos.y - centre(4, 4).1).abs() <= 2 * SUB, "it stayed by the factory");
}

#[test]
fn only_a_factory_of_mobile_units_takes_a_rally_point() {
    let mut world = world();
    let scout = spawn(&mut world, 0, SCOUT, 4, 4);
    world.command(Command::Rally { unit: scout, point: Some(centre(10, 10)) });
    world.step();
    assert_eq!(world.unit(scout).unwrap().rally, None);
}

#[test]
fn intent_orders_replay_exactly() {
    let mut world = world();
    let f = spawn(&mut world, 0, FACTORY, 4, 4);
    let g = spawn(&mut world, 0, GUNNER, 6, 6);
    spawn(&mut world, 1, GUNNER, 18, 18);
    let start = world.clone();
    world.command(Command::Keep { unit: f, kind: GUNNER, count: 2 });
    world.command(Command::Rally { unit: f, point: Some(centre(12, 4)) });
    world.command(Command::Patrol { unit: g, x: centre(20, 20).0, y: centre(20, 20).1 });
    world.command(Command::FallBack { unit: g, percent: 60, x: centre(1, 1).0, y: centre(1, 1).1 });
    run(&mut world, 800);
    let replay = Replay::record(start, world.command_log().to_vec(), 800, 200);
    assert_eq!(hash_of(&replay.seek(800)).value(), hash_of(&world).value());
}
