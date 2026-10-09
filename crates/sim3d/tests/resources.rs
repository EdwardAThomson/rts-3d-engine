use rts_core::hash::hash_of;
use sim3d::economy::{Production, Structure};
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, Event, UnitType, World};

const BUILDER: usize = 0;
const EXTRACTOR: usize = 1;
const TANK: usize = 2;
const GUNNER: usize = 3;

/// Two resources. A builder adds 10 work a tick. An extractor is a 1 by 1 structure taking 100 work and costing
/// nothing; its wreck would be worth 50. A tank's wreck is worth 300 and 120 and takes half its 400 build time, 200 work, to reclaim. A gunner
/// kills a tank in four shots.
fn types() -> Vec<UnitType> {
    let ground = MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 0, altitude: 0, radius: 64 };
    let unit = |movement: MoveClass, production, structure| UnitType {
        movement,
        max_health: 100,
        height: 48,
        vision: 0,
        weapon: None,
        armour: 0,
        production,
        structure,
    };
    let gun =
        Weapon { range: 1536, reload: 10, speed: 128, gravity: 0, damage: 25, splash: 0, scatter: 0, against: vec![] };
    vec![
        unit(ground.clone(), Production { build_power: 10, builds: vec![EXTRACTOR], ..Default::default() }, None),
        unit(
            MoveClass { speed: 0, ..ground.clone() },
            Production { build_time: 100, extracts: true, wreck: vec![50], ..Default::default() },
            Some(Structure { width: 1, depth: 1, max_rise: 16 }),
        ),
        unit(ground.clone(), Production { build_time: 400, wreck: vec![300, 120], ..Default::default() }, None),
        UnitType { weapon: Some(gun), ..unit(ground, Production::default(), None) },
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn world() -> World {
    let mut world = World::new(Heightmap::flat(16, 16, 0), types(), 1);
    world.set_store(0, vec![0, 0], vec![10_000, 10_000]);
    world.add_spot(5, 5, 1, 7);
    world.add_spot(5, 5, 0, 2);
    world
}

#[test]
fn an_extractor_yields_the_spots_under_it_once_finished() {
    let mut world = world();
    let builder = world.spawn(BUILDER, centre(3, 5).0, centre(3, 5).1);
    let (x, y) = centre(9, 9);
    world.spawn(EXTRACTOR, x, y); // Off any spot: yields nothing.
    world.command(Command::Build { unit: builder, kind: EXTRACTOR, cx: 5, cy: 5 });
    let mut finished = false;
    for _ in 0..100 {
        let before = world.store(0).unwrap().amount.clone();
        let events = world.step();
        if !finished {
            assert_eq!(world.store(0).unwrap().amount, before, "nothing comes in before it is finished");
            finished = events.iter().any(|e| matches!(e, Event::Built { .. }));
        } else {
            let after = &world.store(0).unwrap().amount;
            assert_eq!((after[0] - before[0], after[1] - before[1]), (2, 7), "both spots on its cell, each tick");
        }
    }
    assert!(finished);
}

#[test]
fn a_destroyed_unit_leaves_a_wreck_that_reclaims_for_exactly_its_worth() {
    let mut world = world();
    let tank = world.spawn_for(1, TANK, centre(8, 4).0, centre(8, 4).1);
    let gunner = world.spawn(GUNNER, centre(4, 4).0, centre(4, 4).1);
    let builder = world.spawn(BUILDER, centre(2, 12).0, centre(2, 12).1);
    world.command(Command::Attack { unit: gunner, target: tank });
    let mut wreck = None;
    for _ in 0..100 {
        for e in world.step() {
            if let Event::Wrecked { unit, wreck: w } = e {
                assert_eq!(unit, tank);
                wreck = Some(w);
            }
        }
        if wreck.is_some() {
            break;
        }
    }
    let wreck = wreck.expect("the tank left a wreck");
    let w = &world.wrecks()[0];
    assert_eq!((w.id, w.pos.x, w.pos.y, w.work), (wreck, centre(8, 4).0, centre(8, 4).1, 0));

    world.command(Command::Reclaim { unit: builder, wreck });
    let mut done = None;
    let mut gained = Vec::new();
    for _ in 0..300 {
        let tick = world.tick();
        let events = world.step();
        if done.is_none() {
            gained.push(world.store(0).unwrap().amount[0]);
        }
        if events.iter().any(|e| matches!(e, Event::Reclaimed { by, wreck: w } if *by == builder && *w == wreck)) {
            done = Some(tick);
        }
    }
    assert!(done.is_some(), "the wreck was reclaimed");
    assert!(world.wrecks().is_empty(), "and is gone");
    assert_eq!(world.store(0).unwrap().amount, vec![300, 120], "exactly its worth");
    let rising: Vec<_> = gained.windows(2).filter(|w| w[1] > w[0]).collect();
    assert_eq!(rising.len(), 20, "a little each tick: 200 work at 10 a tick");
}

#[test]
fn frames_and_types_without_worth_leave_no_wreck() {
    let mut world = world();
    // Two gunners, worth nothing, shoot it out.
    world.spawn_for(1, GUNNER, centre(8, 12).0, centre(8, 12).1);
    world.spawn(GUNNER, centre(10, 12).0, centre(10, 12).1);
    // A builder puts down an extractor frame (an extractor's wreck would be worth 50) and walks away from it.
    let builder = world.spawn(BUILDER, centre(3, 5).0, centre(3, 5).1);
    world.command(Command::Build { unit: builder, kind: EXTRACTOR, cx: 5, cy: 5 });
    let frame = (0..50)
        .find_map(|_| {
            world.step().into_iter().find_map(|e| match e {
                Event::Placed { unit, .. } => Some(unit),
                _ => None,
            })
        })
        .expect("the frame was placed");
    world.command(Command::Move { unit: builder, x: centre(1, 1).0, y: centre(1, 1).1 });
    world.spawn_for(1, GUNNER, centre(8, 5).0, centre(8, 5).1);
    let mut destroyed = Vec::new();
    for _ in 0..200 {
        for e in world.step() {
            assert!(!matches!(e, Event::Wrecked { .. }), "nothing here leaves a wreck: {e:?}");
            if let Event::Destroyed { unit, .. } = e {
                destroyed.push(unit);
            }
        }
    }
    assert!(destroyed.contains(&frame), "the frame was shot down");
    assert!(destroyed.len() >= 2, "and at least one gunner fell: {destroyed:?}");
    assert!(world.wrecks().is_empty());
}

#[test]
fn a_replay_with_spots_and_wrecks_matches() {
    let start = || {
        let mut world = world();
        world.spawn(BUILDER, centre(3, 5).0, centre(3, 5).1);
        world.spawn_for(1, TANK, centre(8, 4).0, centre(8, 4).1);
        world.spawn(GUNNER, centre(4, 4).0, centre(4, 4).1);
        world
    };
    let mut live = start();
    let mut hashes = Vec::new();
    for tick in 0..300 {
        match tick {
            0 => live.command(Command::Build { unit: 1, kind: EXTRACTOR, cx: 5, cy: 5 }),
            60 => {
                let wreck = live.wrecks()[0].id;
                live.command(Command::Reclaim { unit: 1, wreck });
            }
            _ => {}
        }
        live.step();
        hashes.push(hash_of(&live).value());
    }
    assert!(live.wrecks().is_empty() && live.store(0).unwrap().amount[0] > 300);
    let replay = Replay::record(start(), live.command_log().to_vec(), 300, 64);
    for tick in [1, 64, 100, 299, 300] {
        assert_eq!(hash_of(&replay.seek(tick)).value(), hashes[tick as usize - 1], "tick {tick}");
    }
}

/// Destroys the unit `target` with a gunner of player 0 placed at `from`, and returns its wreck.
fn wreck_of(world: &mut World, target: u32, from: (i32, i32)) -> u32 {
    let gunner = world.spawn_for(0, GUNNER, from.0, from.1);
    world.command(Command::Attack { unit: gunner, target });
    for _ in 0..200 {
        for e in world.step() {
            if let Event::Wrecked { unit, wreck } = e
                && unit == target
            {
                return wreck;
            }
        }
    }
    panic!("no wreck");
}

#[test]
fn a_structures_wreck_blocks_its_footprint_until_it_is_reclaimed() {
    let mut world = world();
    let extractor = world.spawn_for(1, EXTRACTOR, centre(8, 8).0, centre(8, 8).1);
    assert!(world.is_blocked(8, 8));
    let wreck = wreck_of(&mut world, extractor, centre(8, 2));
    assert!(world.is_blocked(8, 8), "the wreck still blocks the cell");
    assert!(!world.site_ok(EXTRACTOR, 8, 8), "and nothing can be built there");

    // A tank driving straight across goes round it.
    let tank = world.spawn(TANK, centre(4, 8).0, centre(4, 8).1);
    world.command(Command::Move { unit: tank, x: centre(12, 8).0, y: centre(12, 8).1 });
    for _ in 0..200 {
        world.step();
        let p = world.unit(tank).unwrap().pos;
        assert!(!((8 * SUB..9 * SUB).contains(&p.x) && (8 * SUB..9 * SUB).contains(&p.y)), "on the wreck at {p:?}");
    }
    let p = world.unit(tank).unwrap().pos;
    assert!(
        p.ground_distance(sim3d::space::Vec3::new(centre(12, 8).0, centre(12, 8).1, 0)) < SUB as u32,
        "and arrives"
    );

    // A builder reclaims it from beside the cell, and the ground opens again.
    let builder = world.spawn(BUILDER, centre(2, 12).0, centre(2, 12).1);
    world.command(Command::Reclaim { unit: builder, wreck });
    for _ in 0..300 {
        world.step();
    }
    assert!(world.wrecks().iter().all(|w| w.id != wreck), "reclaimed");
    assert!(!world.is_blocked(8, 8) && world.site_ok(EXTRACTOR, 8, 8), "and the cell is free");
}

#[test]
fn a_mobile_units_wreck_blocks_nothing() {
    let mut world = world();
    let tank = world.spawn_for(1, TANK, centre(8, 4).0, centre(8, 4).1);
    wreck_of(&mut world, tank, centre(4, 4));
    assert!(!world.is_blocked(8, 4));
    let other = world.spawn(TANK, centre(8, 10).0, centre(8, 10).1);
    world.command(Command::Move { unit: other, x: centre(8, 4).0, y: centre(8, 4).1 });
    for _ in 0..100 {
        world.step();
    }
    let p = world.unit(other).unwrap().pos;
    assert_eq!((p.x, p.y), centre(8, 4), "it drives onto the wreck's own point");
}
