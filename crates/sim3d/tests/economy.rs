use rts_core::hash::hash_of;
use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::world::{Command, Event, UnitType, World};

const FACTORY: usize = 0;
const TANK: usize = 1;
const SOURCE: usize = 2;
const SCOUT: usize = 3;

/// Two resources. A tank costs 1000 of the first and 500 of the second and takes 1000 work; a factory adds 10
/// work a tick, so at full speed a tank takes 100 ticks and costs 10 and 5 a tick. A source produces 5 of the first
/// resource a tick. A scout costs nothing.
fn types() -> Vec<UnitType> {
    let class = |radius| MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 0, altitude: 0, radius };
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
    vec![
        unit(128, Production { build_power: 10, builds: vec![TANK, SCOUT], ..Default::default() }),
        unit(64, Production { cost: vec![1000, 500], build_time: 1000, ..Default::default() }),
        unit(64, Production { produces: vec![5], ..Default::default() }),
        unit(32, Production { build_time: 50, ..Default::default() }),
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

fn world() -> World {
    World::new(Heightmap::flat(16, 16, 0), types(), 1)
}

fn spawn(world: &mut World, kind: usize, cx: i32, cy: i32) -> u32 {
    let (x, y) = centre(cx, cy);
    world.spawn(kind, x, y)
}

/// Run until `factory` builds something or `limit` ticks pass; returns the tick and the new unit.
fn until_built(world: &mut World, factory: u32, limit: u32) -> Option<(u32, u32)> {
    for _ in 0..limit {
        let tick = world.tick();
        for e in world.step() {
            if let Event::Built { by: f, unit } = e
                && f == factory
            {
                return Some((tick, unit));
            }
        }
    }
    None
}

#[test]
fn a_factory_pays_as_it_builds_and_an_item_costs_exactly_its_cost() {
    let mut world = world();
    world.set_store(0, vec![5000, 5000], vec![10_000, 10_000]);
    let factory = spawn(&mut world, FACTORY, 4, 4);
    world.command(Command::Produce { unit: factory, kind: TANK, repeat: false });
    world.step();
    assert_eq!(world.store(0).unwrap().amount, vec![4990, 4995], "one tick's share, not the whole cost");
    let (tick, tank) = until_built(&mut world, factory, 200).expect("the tank is built");
    assert_eq!(tick, 99, "1000 work at 10 a tick");
    assert_eq!(world.store(0).unwrap().amount, vec![4000, 4500]);
    let (t, f) = (world.unit(tank).unwrap(), world.unit(factory).unwrap());
    assert_eq!((t.kind, t.owner), (TANK, 0));
    assert_eq!((t.pos.x, t.pos.y), (f.pos.x, f.pos.y + 128 + 64 + 8), "it rolls out beside the factory");
    assert!(f.queue.is_empty() && f.work == 0);
}

#[test]
fn spending_beyond_income_slows_every_factory_of_that_player_alike() {
    let mut world = world();
    world.set_store(0, vec![0, 100_000], vec![100_000, 100_000]);
    spawn(&mut world, SOURCE, 1, 1);
    let a = spawn(&mut world, FACTORY, 4, 4);
    let b = spawn(&mut world, FACTORY, 10, 4);
    for f in [a, b] {
        world.command(Command::Produce { unit: f, kind: TANK, repeat: false });
    }
    // Two factories ask for 20 of the first resource a tick and the source brings in 5, so both run at about 25%.
    let mut built = Vec::new();
    for _ in 0..500 {
        let tick = world.tick();
        for e in world.step() {
            if let Event::Built { by: factory, .. } = e {
                built.push((tick, factory));
            }
        }
        let store = world.store(0).unwrap();
        assert!(store.amount[0] >= 0, "a store never goes below empty");
        if built.is_empty() {
            assert!(store.rate < 100, "the player is told they are short");
            assert_eq!(world.unit(a).unwrap().work, world.unit(b).unwrap().work, "neither is favoured");
        }
    }
    // 2000 of the resource at 5 a tick is 400 ticks. Shares are equal all the way; at the very end there is one
    // unit too few for both, and the lower id is paid first.
    assert_eq!(built, vec![(398, a), (399, b)]);
}

#[test]
fn income_beyond_capacity_is_lost() {
    let mut world = world();
    world.set_store(0, vec![0], vec![12]);
    spawn(&mut world, SOURCE, 1, 1);
    for _ in 0..5 {
        world.step();
    }
    assert_eq!(world.store(0).unwrap().amount, vec![12]);
}

#[test]
fn queues_repeat_and_clear_and_ignore_what_a_factory_cannot_build() {
    let mut world = world();
    let factory = spawn(&mut world, FACTORY, 8, 4);
    world.command(Command::Produce { unit: factory, kind: SOURCE, repeat: false });
    world.command(Command::Produce { unit: factory, kind: SCOUT, repeat: true });
    let mut built = Vec::new();
    for _ in 0..16 {
        let tick = world.tick();
        built.extend(world.step().into_iter().filter(|e| matches!(e, Event::Built { .. })).map(|_| tick));
    }
    assert_eq!(built, vec![4, 9, 14], "a scout every 5 ticks; the source was never queued");
    world.command(Command::ClearQueue { unit: factory });
    for _ in 0..20 {
        assert!(!world.step().iter().any(|e| matches!(e, Event::Built { .. })));
    }
}

#[test]
fn a_player_with_no_store_only_builds_what_costs_nothing() {
    let mut world = world();
    let factory = spawn(&mut world, FACTORY, 8, 4);
    world.command(Command::Produce { unit: factory, kind: TANK, repeat: false });
    world.command(Command::Produce { unit: factory, kind: SCOUT, repeat: false });
    assert_eq!(until_built(&mut world, factory, 300), None, "the tank waits for resources");
    world.command(Command::ClearQueue { unit: factory });
    world.command(Command::Produce { unit: factory, kind: SCOUT, repeat: false });
    assert!(until_built(&mut world, factory, 10).is_some());
}

#[test]
fn a_replay_of_a_game_with_an_economy_matches() {
    let start = || {
        let mut world = world();
        world.set_store(0, vec![300, 1000], vec![5000, 5000]);
        world.set_store(1, vec![2000, 2000], vec![5000, 5000]);
        let (x, y) = centre(2, 2);
        world.spawn_for(0, SOURCE, x, y);
        let (x, y) = centre(12, 12);
        world.spawn_for(1, FACTORY, x, y);
        world
    };
    let mut live = start();
    let mut hashes = Vec::new();
    for tick in 0..400 {
        match tick {
            0 => live.command(Command::Produce { unit: 2, kind: TANK, repeat: true }),
            150 => live.command(Command::Produce { unit: 2, kind: SCOUT, repeat: false }),
            _ => {}
        }
        live.step();
        hashes.push(hash_of(&live).value());
    }
    assert!(live.units().len() >= 4, "player 1 built tanks");
    let replay = Replay::record(start(), live.command_log().to_vec(), 400, 64);
    for tick in [1, 64, 200, 399, 400] {
        assert_eq!(hash_of(&replay.seek(tick)).value(), hashes[tick as usize - 1], "tick {tick}");
    }
}
