use rts_core::hash::hash_of;
use sim3d::movement::MoveClass;
use sim3d::space::{SUB, Vec3};
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, UnitType, World};

const TANK: usize = 0;
const HIGH_LOB: usize = 1;
const LOW_LOB: usize = 2;
const TARGET: usize = 3;
const SMALL: usize = 4;
const FRAIL_TANK: usize = 5;

fn tracked(radius: i32) -> MoveClass {
    MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius }
}

fn armed(max_health: i32, weapon: Weapon) -> UnitType {
    UnitType { movement: tracked(64), max_health, height: 48, weapon: Some(weapon) }
}

fn cannon() -> Weapon {
    Weapon { range: 2048, reload: 10, speed: 128, gravity: 0, damage: 25, splash: 0, scatter: 0 }
}

fn mortar(gravity: i32) -> Weapon {
    Weapon { range: 2048, reload: 20, speed: 48, gravity, damage: 30, splash: 192, scatter: 0 }
}

fn types() -> Vec<UnitType> {
    vec![
        armed(100, cannon()),
        armed(80, mortar(4)),
        armed(80, mortar(1)),
        UnitType { movement: tracked(64), max_health: 100, height: 48, weapon: None },
        UnitType { movement: tracked(16), max_health: 100, height: 48, weapon: None },
        armed(25, cannon()),
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn spawn(world: &mut World, kind: usize, (x, y): (i32, i32)) -> u32 {
    world.spawn(kind, x, y)
}

/// Run `ticks` ticks and return every event with the tick it happened on.
fn run(world: &mut World, ticks: u32) -> Vec<(u32, Event)> {
    let mut all = Vec::new();
    for _ in 0..ticks {
        let tick = world.tick();
        all.extend(world.step().into_iter().map(|e| (tick, e)));
    }
    all
}

/// An 8 by 3 map with a ridge `peak` high along corner column 4.
fn ridge(peak: i32) -> Heightmap {
    let corners = (0..4).flat_map(|_| (0..9).map(move |cx| if cx == 4 { peak } else { 0 })).collect();
    Heightmap::new(8, 3, corners)
}

#[test]
fn a_tank_destroys_a_target_in_range() {
    let mut world = World::new(Heightmap::flat(8, 5, 0), types(), 1);
    let tank = spawn(&mut world, TANK, centre(1, 2));
    let target = spawn(&mut world, TARGET, centre(5, 2));
    world.command(Command::Attack { unit: tank, target });
    let events = run(&mut world, 60);

    let fired: Vec<u32> = events.iter().filter(|(_, e)| matches!(e, Event::Fired { .. })).map(|(t, _)| *t).collect();
    assert_eq!(fired, vec![0, 10, 20, 30], "one shot per reload until the target is gone");
    let health: Vec<i32> = events
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Damaged { unit, health, .. } if *unit == target => Some(*health),
            _ => None,
        })
        .collect();
    assert_eq!(health, vec![75, 50, 25, 0]);
    // 1024 at 128 a tick: each shot strikes the target's body 7 ticks after it is fired.
    let destroyed: Vec<_> = events.iter().filter(|(_, e)| matches!(e, Event::Destroyed { .. })).collect();
    assert_eq!(destroyed.len(), 1);
    let (tick, Event::Destroyed { unit, by, .. }) = destroyed[0] else { unreachable!() };
    assert_eq!((*unit, *by), (target, tank));
    assert_eq!(*tick, 37);
    assert!(world.unit(target).is_none(), "the destroyed are removed");
    assert_eq!(world.unit(tank).unwrap().target, None, "the attack ends with its target");
    assert!(world.projectiles().is_empty());
}

#[test]
fn a_ridge_blocks_direct_fire_but_a_high_lob_clears_it() {
    let mut world = World::new(ridge(300), types(), 1);
    let tank = spawn(&mut world, TANK, centre(1, 1));
    let mortar = spawn(&mut world, HIGH_LOB, centre(1, 0));
    let target = spawn(&mut world, TARGET, centre(6, 1));
    world.command(Command::Attack { unit: tank, target });
    world.command(Command::Attack { unit: mortar, target });
    let events = run(&mut world, 120);
    let tank_fired = events.iter().any(|(_, e)| matches!(e, Event::Fired { unit, .. } if *unit == tank));
    assert!(!tank_fired, "the tank never sees the target over the ridge");
    let mortar_hits =
        events.iter().filter(|(_, e)| matches!(e, Event::Impact { unit: Some(u), .. } if *u == target)).count();
    assert!(mortar_hits >= 3, "the mortar's shells come down on the target ({mortar_hits} hits)");
    assert!(world.unit(target).is_none(), "and destroy it");
}

#[test]
fn a_low_lob_hits_the_ridge() {
    let mut world = World::new(ridge(300), types(), 1);
    let mortar = spawn(&mut world, LOW_LOB, centre(1, 1));
    let target = spawn(&mut world, TARGET, centre(6, 1));
    world.command(Command::Attack { unit: mortar, target });
    let events = run(&mut world, 40);
    let impacts: Vec<Vec3> = events
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Impact { at, unit: None, .. } => Some(*at),
            _ => None,
        })
        .collect();
    assert!(!impacts.is_empty(), "the flat arc comes down on the ridge");
    for at in impacts {
        assert!((3 * SUB..5 * SUB).contains(&at.x), "it burst on the ridge's slopes, at x {}", at.x);
        assert_eq!(at.z, world.map().sample(at.x, at.y), "a ground burst is on the ground");
    }
    assert_eq!(world.unit(target).unwrap().health, 100, "nothing reached the target");
}

#[test]
fn a_unit_in_the_line_of_fire_takes_the_shot() {
    let mut world = World::new(Heightmap::flat(8, 5, 0), types(), 1);
    let tank = spawn(&mut world, TANK, centre(1, 2));
    let blocker = spawn(&mut world, TARGET, centre(3, 2));
    let target = spawn(&mut world, TARGET, centre(5, 2));
    world.command(Command::Attack { unit: tank, target });
    let events = run(&mut world, 9);
    let impact = events.iter().find_map(|(_, e)| match e {
        Event::Impact { unit, .. } => Some(*unit),
        _ => None,
    });
    assert_eq!(impact, Some(Some(blocker)));
    assert_eq!(world.unit(blocker).unwrap().health, 75);
    assert_eq!(world.unit(target).unwrap().health, 100);
}

#[test]
fn splash_hurts_close_units_fully_and_farther_ones_by_half() {
    let mut world = World::new(Heightmap::flat(12, 5, 0), types(), 1);
    let mortar = spawn(&mut world, HIGH_LOB, centre(1, 2));
    let (tx, ty) = centre(8, 2);
    let target = spawn(&mut world, SMALL, (tx, ty));
    let near = spawn(&mut world, SMALL, (tx, ty + 64));
    let far = spawn(&mut world, SMALL, (tx, ty - 160));
    let away = spawn(&mut world, SMALL, (tx + 300, ty));
    world.command(Command::Attack { unit: mortar, target });
    let events = run(&mut world, 60);
    let first_impact = events.iter().position(|(_, e)| matches!(e, Event::Impact { .. })).unwrap();
    let hurt: Vec<(u32, i32)> = events[first_impact..]
        .iter()
        .take_while(|(t, _)| *t == events[first_impact].0)
        .filter_map(|(_, e)| match e {
            Event::Damaged { unit, damage, .. } => Some((*unit, *damage)),
            _ => None,
        })
        .collect();
    assert_eq!(hurt, vec![(target, 30), (near, 30), (far, 15)]);
    assert_eq!(world.unit(away).unwrap().health, 100);
    assert_eq!(world.unit(mortar).unwrap().health, 80, "never hurt by its own shells");
}

#[test]
fn two_units_that_kill_each_other_both_get_their_shot() {
    let mut world = World::new(Heightmap::flat(8, 5, 0), types(), 1);
    let a = spawn(&mut world, FRAIL_TANK, centre(1, 2));
    let b = spawn(&mut world, FRAIL_TANK, centre(6, 2));
    world.command(Command::Attack { unit: a, target: b });
    world.command(Command::Attack { unit: b, target: a });
    let events = run(&mut world, 20);
    let destroyed: Vec<(u32, u32)> = events
        .iter()
        .filter_map(|(t, e)| match e {
            Event::Destroyed { unit, .. } => Some((*t, *unit)),
            _ => None,
        })
        .collect();
    assert_eq!(destroyed.len(), 2);
    assert_eq!(destroyed[0].0, destroyed[1].0, "both die on the same tick");
    assert!(world.units().is_empty());
}

#[test]
fn an_attacker_out_of_range_closes_in_then_fires() {
    let mut world = World::new(Heightmap::flat(24, 5, 0), types(), 1);
    let tank = spawn(&mut world, TANK, centre(1, 2));
    let target = spawn(&mut world, TARGET, centre(20, 2));
    world.command(Command::Attack { unit: tank, target });
    let mut first_shot = None;
    for _ in 0..300 {
        for e in world.step() {
            if let Event::Fired { from, aim, .. } = e
                && first_shot.is_none()
            {
                first_shot = Some(from.ground_distance(aim));
            }
        }
    }
    let d = first_shot.expect("it fired once in range") as i32;
    assert!((2048 - 32..=2048).contains(&d), "it stopped as soon as it was in range: {d}");
    assert!(world.unit(target).is_none());
}

#[test]
fn scatter_moves_the_aim_point_within_its_radius_and_follows_the_seed() {
    let shots = |seed| {
        let mut t = types();
        t[TANK].weapon.as_mut().unwrap().scatter = 48;
        t[TARGET].max_health = 10_000;
        let mut world = World::new(Heightmap::flat(8, 5, 0), t, seed);
        let tank = spawn(&mut world, TANK, centre(1, 2));
        let target = spawn(&mut world, TARGET, centre(5, 2));
        world.command(Command::Attack { unit: tank, target });
        let (tx, ty) = centre(5, 2);
        run(&mut world, 100)
            .into_iter()
            .filter_map(|(_, e)| match e {
                Event::Fired { aim, .. } => Some((aim.x - tx, aim.y - ty)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let a = shots(1);
    assert_eq!(a.len(), 10);
    assert!(a.iter().all(|&(dx, dy)| dx * dx + dy * dy <= 48 * 48));
    assert!(a.iter().any(|&o| o != (0, 0)), "the aim does scatter");
    assert_eq!(a, shots(1), "the same seed gives the same shots");
    assert_ne!(a, shots(2));
}

/// Two squads on a ridge map trading fire, with orders at different ticks.
fn battle() -> World {
    let mut world = World::new(ridge(120), types(), 9);
    spawn(&mut world, TANK, centre(0, 0));
    spawn(&mut world, HIGH_LOB, centre(0, 1));
    spawn(&mut world, TANK, centre(1, 2));
    spawn(&mut world, TANK, centre(7, 0));
    spawn(&mut world, LOW_LOB, centre(7, 1));
    spawn(&mut world, TARGET, centre(6, 2));
    world
}

fn battle_orders(world: &mut World) {
    match world.tick() {
        0 => {
            for (unit, target) in [(1, 4), (2, 6), (3, 5), (4, 1), (5, 2)] {
                world.command(Command::Attack { unit, target });
            }
        }
        25 => world.command(Command::Move { unit: 6, x: 2 * SUB, y: 2 * SUB }),
        60 => world.command(Command::Attack { unit: 3, target: 6 }),
        _ => {}
    }
}

#[test]
fn a_replay_of_a_battle_matches_tick_for_tick() {
    let mut live = battle();
    let mut hashes = Vec::new();
    let mut destroyed = Vec::new();
    for _ in 0..300 {
        battle_orders(&mut live);
        for e in live.step() {
            if let Event::Destroyed { unit, .. } = e {
                destroyed.push(unit);
            }
        }
        hashes.push(hash_of(&live).value());
    }
    assert_eq!(destroyed, vec![1, 4, 5, 2, 6], "both tanks on the ridge, both mortars, then the target");

    let mut replay = battle();
    let log = live.command_log().to_vec();
    for (i, expected) in hashes.iter().enumerate() {
        let tick = replay.tick();
        for logged in log.iter().filter(|l| l.tick == tick) {
            replay.command(logged.command.clone());
        }
        replay.step();
        assert_eq!(hash_of(&replay).value(), *expected, "replay diverged at tick {i}");
    }
    // The golden hash pins today's combat rules. If a change moves it, say so and update it on purpose.
    assert_eq!(hash_of(&live).hex(), "d048bd55");
}
