use rts_core::hash::hash_of;
use rts_core::rng::{random_int, seed_state};
use sim3d::economy::Production;
use sim3d::movement::MoveClass;
use sim3d::replay::Replay;
use sim3d::space::SUB;
use sim3d::terrain::Heightmap;
use sim3d::weapon::Weapon;
use sim3d::world::{Command, UnitType, World};

const TICKS: u32 = 600;
const EVERY: u32 = 100;

fn types() -> Vec<UnitType> {
    let tracked = MoveClass { speed: 32, max_slope: Some(64), climb_slowdown: 50, altitude: 0, radius: 64 };
    let gun =
        Weapon { range: 1536, reload: 12, speed: 96, gravity: 0, damage: 20, splash: 0, scatter: 24, against: vec![] };
    let lob = Weapon {
        range: 2048,
        reload: 25,
        speed: 48,
        gravity: 3,
        damage: 25,
        splash: 160,
        scatter: 64,
        against: vec![],
    };
    vec![
        UnitType {
            movement: tracked.clone(),
            max_health: 120,
            height: 48,
            vision: 0,
            weapon: Some(gun),
            armour: 0,
            production: Production::default(),
            structure: None,
        },
        UnitType {
            movement: tracked,
            max_health: 80,
            height: 48,
            vision: 0,
            weapon: Some(lob),
            armour: 0,
            production: Production::default(),
            structure: None,
        },
    ]
}

fn centre(cx: i32, cy: i32) -> (i32, i32) {
    (cx * SUB + SUB / 2, cy * SUB + SUB / 2)
}

/// Two sides of six on rolling ground, out of range of each other at the start.
fn start() -> World {
    let mut rng = seed_state(11);
    let corners = (0..25 * 17).map(|_| random_int(&mut rng, 48) as i32).collect();
    let mut world = World::new(Heightmap::new(24, 16, corners), types(), 5);
    for owner in 0..2u8 {
        for i in 0..6 {
            let cx = if owner == 0 { 2 + i % 2 } else { 20 + i % 2 };
            let (x, y) = centre(cx, 3 + 2 * i);
            world.spawn_for(owner, (i % 2) as usize, x, y);
        }
    }
    world
}

/// Orders given during the recorded game: side 0 attack-moves across, side 1 holds, then one of its units
/// counter-attacks and another is pulled back.
fn orders(world: &mut World) {
    match world.tick() {
        0 => {
            for id in 1..=6 {
                world.command(Command::AttackMove { unit: id, x: centre(18, 8).0, y: centre(18, 8).1 });
            }
        }
        150 => world.command(Command::Attack { unit: 7, target: 1 }),
        151 => world.command(Command::Move { unit: 12, x: centre(23, 15).0, y: centre(23, 15).1 }),
        320 => world.command(Command::Stop { unit: 3 }),
        _ => {}
    }
}

/// The recorded game and the hash at the start of every tick from 0 to `TICKS`.
fn recorded() -> (World, Vec<u32>) {
    let mut live = start();
    let mut hashes = vec![hash_of(&live).value()];
    for _ in 0..TICKS {
        orders(&mut live);
        live.step();
        hashes.push(hash_of(&live).value());
    }
    (live, hashes)
}

#[test]
fn seeking_to_any_tick_gives_the_same_world_as_playing_there() {
    let (live, hashes) = recorded();
    assert!(live.units().len() < 12, "the recorded game has a real fight");
    let replay = Replay::record(start(), live.command_log().to_vec(), TICKS, EVERY);
    // Forwards, backwards, on and between snapshots, and at both ends.
    for tick in [0, 1, 99, 100, 101, 350, 600, 599, 50, 151, 150, 0, 320, 321] {
        let world = replay.seek(tick);
        assert_eq!(world.tick(), tick);
        assert_eq!(hash_of(&world).value(), hashes[tick as usize], "seek to tick {tick} differs from the game");
    }
}

#[test]
fn a_player_can_take_over_from_any_tick_and_their_game_is_a_replay_too() {
    let (live, hashes) = recorded();
    let replay = Replay::record(start(), live.command_log().to_vec(), TICKS, EVERY);

    // Take over at tick 200 and pull side 0 back instead.
    let mut mine = replay.seek(200);
    for id in 1..=6 {
        if mine.unit(id).is_some() {
            mine.command(Command::Move { unit: id, x: centre(1, 8).0, y: centre(1, 8).1 });
        }
    }
    for _ in 200..400 {
        mine.step();
    }
    assert_ne!(hash_of(&mine).value(), hashes[400], "the new line of play differs");
    assert_eq!(hash_of(&replay.seek(400)).value(), hashes[400], "the replay is untouched");

    // The taken-over game's log is the replay's log to tick 200 and then the new orders, and replays exactly.
    let log = mine.command_log().to_vec();
    assert!(log.iter().all(|l| l.tick <= 200));
    assert_eq!(log.iter().filter(|l| l.tick < 200).count(), replay.log().iter().filter(|l| l.tick < 200).count());
    let again = Replay::record(start(), log, 400, EVERY);
    assert_eq!(hash_of(&again.seek(400)).value(), hash_of(&mine).value());
}
