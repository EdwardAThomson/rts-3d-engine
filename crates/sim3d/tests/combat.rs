use rts_core::hash::hash_of;
use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::space::{SUB, Vec3};
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, MoveEnd, UnitType, World};

const TANK: usize = 0;
const HIGH_LOB: usize = 1;
const LOW_LOB: usize = 2;
const TARGET: usize = 3;
const SMALL: usize = 4;
const FRAIL_TANK: usize = 5;

fn tracked(radius: i32) -> MoveClass {
    MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius, turn: 0 }
}

fn armed(max_health: i32, weapon: Weapon) -> UnitType {
    UnitType {
        movement: tracked(64),
        max_health,
        height: 48,
        vision: 0,
        weapon: Some(weapon),
        armour: 0,
        production: Production::default(),
        structure: None,
    }
}

fn cannon() -> Weapon {
    Weapon {
        range: 2048,
        reload: 10,
        speed: 128,
        gravity: 0,
        damage: 25,
        splash: 0,
        scatter: 0,
        against: vec![],
        turret: Some(0),
    }
}

fn mortar(gravity: i32) -> Weapon {
    Weapon {
        range: 2048,
        reload: 20,
        speed: 48,
        gravity,
        damage: 30,
        splash: 192,
        scatter: 0,
        against: vec![],
        turret: Some(0),
    }
}

fn types() -> Vec<UnitType> {
    vec![
        armed(100, cannon()),
        armed(80, mortar(4)),
        armed(80, mortar(1)),
        UnitType {
            movement: tracked(64),
            max_health: 100,
            height: 48,
            vision: 0,
            weapon: None,
            armour: 0,
            production: Production::default(),
            structure: None,
        },
        UnitType {
            movement: tracked(16),
            max_health: 100,
            height: 48,
            vision: 0,
            weapon: None,
            armour: 0,
            production: Production::default(),
            structure: None,
        },
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
    let target = world.spawn_for(1, SMALL, tx, ty);
    let near = world.spawn_for(1, SMALL, tx, ty + 64);
    let far = world.spawn_for(1, SMALL, tx, ty - 160);
    let away = world.spawn_for(1, SMALL, tx + 300, ty);
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

#[test]
fn own_side_splash_does_half_damage() {
    let mut world = World::new(Heightmap::flat(12, 5, 0), types(), 1);
    let mortar = spawn(&mut world, HIGH_LOB, centre(1, 2));
    let (tx, ty) = centre(8, 2);
    let target = world.spawn_for(1, SMALL, tx, ty);
    let friend = spawn(&mut world, SMALL, (tx, ty + 64));
    world.command(Command::Attack { unit: mortar, target });
    run(&mut world, 40);
    assert_eq!(world.unit(friend).unwrap().health, 100 - 15, "half of the full-band 30");
}

#[test]
fn idle_units_open_fire_on_enemies_in_range_by_themselves() {
    let mut world = World::new(Heightmap::flat(12, 5, 0), types(), 1);
    let ours = spawn(&mut world, TANK, centre(1, 2));
    let friend = spawn(&mut world, TARGET, centre(3, 2 - 1));
    let near = world.spawn_for(1, TARGET, centre(6, 2).0, centre(6, 2).1);
    let far = world.spawn_for(1, TARGET, centre(10, 2).0, centre(10, 2).1);
    let events = run(&mut world, 200);
    let targets: Vec<u32> = events
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Damaged { unit, .. } => Some(*unit),
            _ => None,
        })
        .collect();
    assert!(!targets.contains(&friend), "never its own side");
    assert!(!targets.contains(&far), "never beyond its range of 8 cells");
    assert_eq!(targets.len(), 4, "the nearer enemy, four shots to destroy");
    assert!(world.unit(near).is_none());
    assert!(world.unit(far).is_some());
    let u = world.unit(ours).unwrap();
    assert_eq!((u.target, u.goal), (None, None), "it does not go after the far one");
}

#[test]
fn a_self_picked_target_is_dropped_when_it_drives_out_of_range() {
    let mut t = types();
    t[TARGET].max_health = 10_000;
    let mut world = World::new(Heightmap::flat(24, 5, 0), t, 1);
    let ours = spawn(&mut world, TANK, centre(1, 2));
    let (ex, ey) = centre(8, 2);
    let enemy = world.spawn_for(1, TARGET, ex, ey);
    run(&mut world, 20);
    assert_eq!(world.unit(ours).unwrap().target, Some(enemy), "it picked the enemy");
    world.command(Command::Move { unit: enemy, x: centre(22, 2).0, y: ey });
    run(&mut world, 200);
    let u = world.unit(ours).unwrap();
    assert_eq!((u.target, u.goal), (None, None), "it let the enemy go and stayed put");
    assert_eq!((u.pos.x, u.pos.y), centre(1, 2));
}

#[test]
fn direct_fire_picks_the_nearest_enemy_it_can_actually_hit() {
    // The tank stands on the near slope of a ridge. One enemy is just over the crest, closer but hidden; the
    // other is farther back on open ground.
    let mut world = World::new(ridge(300), types(), 1);
    let ours = spawn(&mut world, TANK, centre(3, 1));
    let (hx, hy) = (4 * SUB + SUB / 2, centre(3, 1).1);
    let hidden = world.spawn_for(1, TARGET, hx, hy);
    let (ox, oy) = centre(0, 1);
    let open = world.spawn_for(1, TARGET, ox, oy);
    run(&mut world, 9);
    assert_eq!(world.unit(ours).unwrap().target, Some(open));
    assert_eq!(world.unit(hidden).unwrap().health, 100);
}

/// Two squads on a ridge map trading fire, with orders at different ticks.
fn battle() -> World {
    let mut world = World::new(ridge(120), types(), 9);
    spawn(&mut world, TANK, centre(0, 0));
    spawn(&mut world, HIGH_LOB, centre(0, 1));
    spawn(&mut world, TANK, centre(1, 2));
    for (kind, (x, y)) in [(TANK, centre(7, 0)), (LOW_LOB, centre(7, 1)), (TARGET, centre(6, 2))] {
        world.spawn_for(1, kind, x, y);
    }
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
    assert_eq!(hash_of(&live).hex(), "5b203108");
}

const LIGHT: usize = 0;
const HEAVY: usize = 1;
const AIRFRAME: usize = 2;

/// A cannon that does full damage to light armour, 40% to heavy and nothing to aircraft, and three targets,
/// one of each armour class.
fn armoured() -> Vec<UnitType> {
    let gun = Weapon { against: vec![100, 40, 0], ..cannon() };
    let target = |armour, altitude| UnitType {
        movement: MoveClass { altitude, ..tracked(64) },
        max_health: 100,
        height: 48,
        vision: 0,
        weapon: None,
        armour,
        production: Production::default(),
        structure: None,
    };
    vec![armed(100, gun), target(LIGHT, 0), target(HEAVY, 0), target(AIRFRAME, 0)]
}

#[test]
fn armour_scales_the_damage_a_weapon_deals() {
    let mut world = World::new(Heightmap::flat(8, 8, 0), armoured(), 1);
    let gun = spawn(&mut world, 0, centre(1, 1));
    let light = world.spawn_for(1, 1, centre(5, 1).0, centre(5, 1).1);
    let heavy = world.spawn_for(1, 2, centre(1, 5).0, centre(1, 5).1);
    world.command(Command::Attack { unit: gun, target: light });
    run(&mut world, 9);
    world.command(Command::Attack { unit: gun, target: heavy });
    run(&mut world, 20);
    assert_eq!(world.unit(light).unwrap().health, 75, "full 25 against light armour");
    assert_eq!(world.unit(heavy).unwrap().health, 80, "two shots of 10: 40% of 25 against heavy armour");
}

#[test]
fn units_never_pick_a_target_their_weapon_cannot_hurt() {
    let mut world = World::new(Heightmap::flat(12, 5, 0), armoured(), 1);
    let gun = spawn(&mut world, 0, centre(1, 2));
    let immune = world.spawn_for(1, 3, centre(3, 2).0, centre(3, 2).1);
    let heavy = world.spawn_for(1, 2, centre(8, 2).0, centre(8, 2).1);
    run(&mut world, 9);
    assert_eq!(world.unit(gun).unwrap().target, Some(heavy), "the nearer enemy is immune, so it takes the far one");
    // Even an ordered attack on it does nothing.
    world.command(Command::Attack { unit: gun, target: immune });
    run(&mut world, 60);
    assert_eq!(world.unit(immune).unwrap().health, 100);
}

/// A 24 by 5 map with a tank on the west edge and an unarmed enemy off to the side of its road east, out of
/// range at the start.
fn roadside() -> (World, u32, u32) {
    let mut world = World::new(Heightmap::flat(24, 5, 0), types(), 1);
    let tank = spawn(&mut world, TANK, centre(1, 2));
    let (x, y) = centre(14, 4);
    let enemy = world.spawn_for(1, TARGET, x, y);
    (world, tank, enemy)
}

#[test]
fn an_attack_move_stops_to_destroy_an_enemy_on_the_way_then_drives_on() {
    let goal = centre(22, 2);
    let (mut plain, tank, enemy) = roadside();
    plain.command(Command::Move { unit: tank, x: goal.0, y: goal.1 });
    let events = run(&mut plain, 300);
    assert!(!events.iter().any(|(_, e)| matches!(e, Event::Fired { .. })), "a plain move ignores enemies");
    assert_eq!(plain.unit(enemy).unwrap().health, 100);

    let (mut world, tank, enemy) = roadside();
    world.command(Command::AttackMove { unit: tank, x: goal.0, y: goal.1 });
    let mut fired = Vec::new();
    let mut held = Vec::new();
    let (mut destroyed, mut arrived) = (None, None);
    for _ in 0..400 {
        let tick = world.tick();
        for e in world.step() {
            match e {
                Event::Fired { unit, .. } if unit == tank => fired.push(tick),
                Event::Destroyed { unit, by, .. } if unit == enemy => destroyed = Some((tick, by)),
                Event::MoveEnded { unit, reason, x, y } if unit == tank => arrived = Some((tick, reason, x, y)),
                _ => {}
            }
        }
        if !fired.is_empty() && destroyed.is_none() {
            held.push(world.unit(tank).unwrap().pos);
        }
    }
    // Four shots of 25 destroy it. Each takes 14 ticks to land and the cannon reloads in 10, so a fifth is
    // already on its way when the fourth strikes.
    assert_eq!(fired, vec![47, 57, 67, 77, 87]);
    assert!(fired[0] > 0, "the enemy was out of range at the start");
    assert_eq!(destroyed.map(|(_, by)| by), Some(tank));
    assert!(held.windows(2).all(|w| w[0] == w[1]), "the tank stood still while it fought");
    let (tick, reason, x, y) = arrived.expect("the tank drove on and arrived");
    assert_eq!((reason, (x, y)), (MoveEnd::Arrived, goal));
    assert!(tick > destroyed.unwrap().0, "it arrived after the fight");
    let unit = world.unit(tank).unwrap();
    assert!(!unit.hunt && unit.goal.is_none(), "the attack-move is over");
}

#[test]
fn a_replay_of_an_attack_move_matches() {
    let (mut live, tank, _) = roadside();
    live.command(Command::AttackMove { unit: tank, x: centre(22, 2).0, y: centre(22, 2).1 });
    let mut hashes = Vec::new();
    for _ in 0..200 {
        live.step();
        hashes.push(hash_of(&live).value());
    }
    let (mut replay, _, _) = roadside();
    let log = live.command_log().to_vec();
    for (i, expected) in hashes.iter().enumerate() {
        let tick = replay.tick();
        for logged in log.iter().filter(|l| l.tick == tick) {
            replay.command(logged.command.clone());
        }
        replay.step();
        assert_eq!(hash_of(&replay).value(), *expected, "replay diverged at tick {i}");
    }
}

/// A shot and its mirror image, turned half a turn about the middle of the map, fly and land as mirror images, so
/// no side of a mirrored map shoots straighter.
#[test]
fn a_shot_and_its_mirror_image_land_as_mirror_images() {
    let size = 16;
    let mirror = |(x, y): (i32, i32)| (size * SUB - x, size * SUB - y);
    for kind in [TANK, HIGH_LOB, LOW_LOB] {
        let mut a = World::new(Heightmap::flat(size, size, 0), types(), 1);
        let mut b = World::new(Heightmap::flat(size, size, 0), types(), 1);
        let (gun, target) = ((1000, 1300), (2401, 2003));
        let ga = spawn(&mut a, kind, gun);
        let ta = spawn(&mut a, TARGET, target);
        let gb = spawn(&mut b, kind, mirror(gun));
        let tb = spawn(&mut b, TARGET, mirror(target));
        a.command(Command::Attack { unit: ga, target: ta });
        b.command(Command::Attack { unit: gb, target: tb });
        let landed = |events: Vec<(u32, Event)>, flip: bool| -> Vec<(u32, (i32, i32))> {
            let at = |v: sim3d::space::Vec3| if flip { mirror((v.x, v.y)) } else { (v.x, v.y) };
            events
                .into_iter()
                .filter_map(|(t, e)| matches!(e, Event::Impact { .. }).then(|| (t, e)))
                .map(|(t, e)| match e {
                    Event::Impact { at: p, .. } => (t, at(p)),
                    _ => unreachable!(),
                })
                .collect()
        };
        let (la, lb) = (landed(run(&mut a, 200), false), landed(run(&mut b, 200), true));
        assert!(!la.is_empty());
        assert_eq!(la, lb, "weapon kind {kind}");
    }
}
